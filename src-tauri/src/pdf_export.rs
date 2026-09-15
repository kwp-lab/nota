use anyhow::{Context, Result, bail};
use parking_lot::Mutex;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::time::Duration;
use tauri::Webview;

const PDF_EXPORT_TIMEOUT: Duration = Duration::from_secs(60);

fn validate_pdf_destination(value: &str) -> Result<PathBuf> {
    let value = value.trim();
    if value.is_empty() {
        bail!("请选择 PDF 导出位置");
    }
    let destination = PathBuf::from(value);
    if !destination.is_absolute() {
        bail!("PDF 导出路径必须是绝对路径");
    }
    if !destination
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
    {
        bail!("PDF 导出文件必须使用 .pdf 扩展名");
    }
    let parent = destination
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .context("PDF 导出路径缺少父目录")?;
    if !parent.is_dir() {
        bail!("PDF 导出目录不存在");
    }
    if destination.is_dir() {
        bail!("PDF 导出位置不能是目录");
    }
    Ok(destination)
}

pub(crate) fn validate_exported_pdf(value: &str) -> Result<PathBuf> {
    let destination = validate_pdf_destination(value)?;
    if !destination.is_file() {
        bail!("找不到已导出的 PDF 文件");
    }
    Ok(destination)
}

type PdfCompletion = Arc<Mutex<Option<mpsc::Sender<Result<()>>>>>;

fn finish(completion: &PdfCompletion, result: Result<()>) {
    if let Some(sender) = completion.lock().take() {
        let _ = sender.send(result);
    }
}

#[cfg(windows)]
fn begin_windows_pdf_export(
    webview: Webview,
    destination: &Path,
    completion: PdfCompletion,
) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT, ICoreWebView2_7, ICoreWebView2Environment6,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use webview2_windows_core::{Interface, PCWSTR};

    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let setup_completion = Arc::clone(&completion);
    webview
        .with_webview(move |platform_webview| {
            let result = (|| unsafe {
                let core_webview = platform_webview
                    .controller()
                    .CoreWebView2()
                    .context("无法读取当前 WebView2 页面")?;
                let printable_webview = core_webview
                    .cast::<ICoreWebView2_7>()
                    .context("当前 WebView2 Runtime 不支持 PDF 导出")?;
                let print_environment = platform_webview
                    .environment()
                    .cast::<ICoreWebView2Environment6>()
                    .context("当前 WebView2 Runtime 不支持打印设置")?;
                let settings = print_environment
                    .CreatePrintSettings()
                    .context("无法创建 PDF 打印设置")?;

                settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT)?;
                settings.SetPageWidth(8.27)?;
                settings.SetPageHeight(11.69)?;
                settings.SetMarginTop(0.0)?;
                settings.SetMarginBottom(0.0)?;
                settings.SetMarginLeft(0.0)?;
                settings.SetMarginRight(0.0)?;
                settings.SetScaleFactor(1.0)?;
                settings.SetShouldPrintBackgrounds(true)?;
                settings.SetShouldPrintHeaderAndFooter(false)?;

                let callback_completion = Arc::clone(&completion);
                let callback =
                    PrintToPdfCompletedHandler::create(Box::new(move |status, succeeded| {
                        let result = match status {
                            Ok(()) if succeeded => Ok(()),
                            Ok(()) => Err(anyhow::anyhow!("WebView2 未能完成 PDF 导出")),
                            Err(error) => {
                                Err(anyhow::Error::new(error).context("WebView2 PDF 导出失败"))
                            }
                        };
                        finish(&callback_completion, result);
                        Ok(())
                    }));
                printable_webview
                    .PrintToPdf(PCWSTR(destination.as_ptr()), &settings, &callback)
                    .context("无法启动 WebView2 PDF 导出")?;
                Ok(())
            })();
            if let Err(error) = result {
                finish(&setup_completion, Err(error));
            }
        })
        .context("无法访问当前 Nota WebView")?;
    Ok(())
}

pub async fn export_current_webview_pdf(webview: Webview, path: &str) -> Result<()> {
    let destination = validate_pdf_destination(path)?;
    let (sender, receiver) = mpsc::channel();
    let completion = Arc::new(Mutex::new(Some(sender)));

    #[cfg(windows)]
    begin_windows_pdf_export(webview, &destination, completion)?;

    #[cfg(not(windows))]
    {
        let _ = webview;
        let _ = destination;
        let _ = completion;
        bail!("PDF 导出目前仅支持 Windows");
    }

    let received =
        tauri::async_runtime::spawn_blocking(move || receiver.recv_timeout(PDF_EXPORT_TIMEOUT))
            .await
            .context("等待 PDF 导出任务失败")?;
    match received {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => bail!("PDF 导出超时，请重试"),
        Err(mpsc::RecvTimeoutError::Disconnected) => bail!("PDF 导出意外中断"),
    }
}

#[cfg(test)]
mod tests {
    use super::{validate_exported_pdf, validate_pdf_destination};
    use std::fs;

    #[test]
    fn pdf_destination_requires_an_absolute_pdf_path() {
        assert!(validate_pdf_destination("").is_err());
        assert!(validate_pdf_destination("report.pdf").is_err());
        assert!(validate_pdf_destination("C:\\Exports\\report.txt").is_err());
    }

    #[test]
    fn pdf_destination_accepts_an_existing_parent_without_creating_output() {
        let directory =
            std::env::temp_dir().join(format!("nota-pdf-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("Meeting.PDF");

        assert_eq!(
            validate_pdf_destination(destination.to_string_lossy().as_ref()).unwrap(),
            destination
        );
        assert!(!destination.exists());

        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn exported_pdf_requires_an_existing_pdf_file() {
        let directory =
            std::env::temp_dir().join(format!("nota-pdf-reveal-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&directory).unwrap();
        let destination = directory.join("Meeting.PDF");

        assert!(validate_exported_pdf(destination.to_string_lossy().as_ref()).is_err());
        fs::write(&destination, b"%PDF-test").unwrap();
        assert_eq!(
            validate_exported_pdf(destination.to_string_lossy().as_ref()).unwrap(),
            destination
        );
        assert!(
            validate_exported_pdf(directory.join("Meeting.txt").to_string_lossy().as_ref())
                .is_err()
        );

        fs::remove_dir_all(directory).unwrap();
    }
}
