use crate::logging::{self, Field, FieldKey};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Utc};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RecoverableFile {
    pub id: String,
    pub path: PathBuf,
    pub created_at: DateTime<Utc>,
    pub size_bytes: u64,
}

pub fn list_recoverable_files(directory: &Path) -> Result<Vec<RecoverableFile>> {
    let mut files = Vec::new();
    if !directory.exists() {
        return Ok(files);
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("ogg")
            || !path
                .file_name()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.ends_with(".partial.ogg"))
        {
            continue;
        }
        let metadata = entry.metadata()?;
        let id = path
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .trim_end_matches(".partial.ogg")
            .to_owned();
        let created_at = metadata
            .created()
            .or_else(|_| metadata.modified())
            .map(DateTime::<Utc>::from)
            .unwrap_or_else(|_| Utc::now());
        files.push(RecoverableFile {
            id,
            path,
            created_at,
            size_bytes: metadata.len(),
        });
    }
    files.sort_by_key(|file| std::cmp::Reverse(file.created_at));
    Ok(files)
}

pub fn recover_ogg_file(partial: &Path, destination: &Path) -> Result<u64> {
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(partial)
        .with_context(|| format!("无法打开恢复文件 {}", partial.display()))?;
    let (valid_end, last_page) = scan_pages(&mut file)?;
    if valid_end == 0 {
        bail!("文件中没有完整的 Ogg 页面");
    }
    file.set_len(valid_end)?;
    if let Some(page) = last_page.filter(|page| !page.end_stream) {
        file.seek(SeekFrom::End(0))?;
        let eos = eos_page(page.serial, page.sequence.wrapping_add(1), page.granule);
        file.write_all(&eos)?;
    }
    file.sync_all()?;
    drop(file);
    move_verified(partial, destination)?;
    Ok(std::fs::metadata(destination)?.len())
}

pub fn move_verified(source: &Path, destination: &Path) -> Result<()> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match rename_with_retry(source, destination) {
        Ok(()) => return Ok(()),
        Err(_) => {
            logging::info(
                "recording_storage",
                "cross_volume_copy_fallback",
                &[Field::text(FieldKey::Reason, "direct_move_failed")],
            );
        }
    }
    let temporary = destination.with_extension("ogg.copying");
    std::fs::copy(source, &temporary)
        .with_context(|| format!("无法把恢复文件复制到临时目标 {}", temporary.display()))?;
    // FlushFileBuffers on Windows requires a handle opened for writing.
    // File::open is read-only and returns ERROR_ACCESS_DENIED on some NTFS
    // volumes even though the copy itself succeeded.
    let temporary_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("无法打开临时目标 {}", temporary.display()))?;
    temporary_file
        .sync_all()
        .with_context(|| format!("无法持久化临时目标 {}", temporary.display()))?;
    drop(temporary_file);

    let source_size = std::fs::metadata(source)?.len();
    let destination_size = std::fs::metadata(&temporary)?.len();
    if source_size != destination_size {
        let _ = std::fs::remove_file(&temporary);
        bail!("跨磁盘复制校验失败");
    }

    rename_with_retry(&temporary, destination).with_context(|| {
        format!(
            "临时录音已完整保存在 {}，但 Windows 持续拒绝将它改名为 {}",
            temporary.display(),
            destination.display()
        )
    })?;

    if remove_with_retry(source).is_err() {
        // The destination is already committed and verified. A stale recovery
        // copy is preferable to reporting a completed recording as failed.
        logging::warn(
            "recording_storage",
            "recovery_source_cleanup_failed",
            &[Field::text(FieldKey::ErrorCode, "source_cleanup_failed")],
        );
    }
    Ok(())
}

fn rename_with_retry(source: &Path, destination: &Path) -> std::io::Result<()> {
    const RETRY_DELAYS_MS: [u64; 7] = [0, 25, 50, 100, 200, 400, 800];
    let mut last_error = None;
    for (attempt, delay_ms) in RETRY_DELAYS_MS.into_iter().enumerate() {
        if delay_ms != 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }
        match std::fs::rename(source, destination) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt + 1 < RETRY_DELAYS_MS.len()
                    && is_transient_file_operation_error(&error) =>
            {
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("rename retry loop always records an error"))
}

fn remove_with_retry(path: &Path) -> std::io::Result<()> {
    const RETRY_DELAYS_MS: [u64; 5] = [0, 25, 75, 200, 500];
    let mut last_error = None;
    for (attempt, delay_ms) in RETRY_DELAYS_MS.into_iter().enumerate() {
        if delay_ms != 0 {
            thread::sleep(Duration::from_millis(delay_ms));
        }
        match std::fs::remove_file(path) {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt + 1 < RETRY_DELAYS_MS.len()
                    && is_transient_file_operation_error(&error) =>
            {
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }
    Err(last_error.expect("remove retry loop always records an error"))
}

fn is_transient_file_operation_error(error: &std::io::Error) -> bool {
    matches!(error.raw_os_error(), Some(5 | 32 | 33))
        || error.kind() == std::io::ErrorKind::PermissionDenied
}

#[derive(Clone, Copy)]
struct LastPage {
    serial: u32,
    sequence: u32,
    granule: u64,
    end_stream: bool,
}

fn scan_pages(file: &mut File) -> Result<(u64, Option<LastPage>)> {
    file.seek(SeekFrom::Start(0))?;
    let length = file.metadata()?.len();
    let mut offset = 0u64;
    let mut last = None;
    while offset + 27 <= length {
        file.seek(SeekFrom::Start(offset))?;
        let mut header = [0u8; 27];
        if file.read_exact(&mut header).is_err() || &header[0..4] != b"OggS" {
            break;
        }
        let segment_count = header[26] as usize;
        let mut lacing = vec![0u8; segment_count];
        if file.read_exact(&mut lacing).is_err() {
            break;
        }
        let body_length: u64 = lacing.iter().map(|value| *value as u64).sum();
        let page_length = 27 + segment_count as u64 + body_length;
        if offset + page_length > length {
            break;
        }
        let mut page = vec![0u8; page_length as usize];
        file.seek(SeekFrom::Start(offset))?;
        file.read_exact(&mut page)?;
        let expected = u32::from_le_bytes(page[22..26].try_into().unwrap());
        page[22..26].fill(0);
        if ogg_crc(&page) != expected {
            break;
        }
        last = Some(LastPage {
            granule: u64::from_le_bytes(header[6..14].try_into().unwrap()),
            serial: u32::from_le_bytes(header[14..18].try_into().unwrap()),
            sequence: u32::from_le_bytes(header[18..22].try_into().unwrap()),
            end_stream: header[5] & 0x04 != 0,
        });
        offset += page_length;
    }
    Ok((offset, last))
}

fn eos_page(serial: u32, sequence: u32, granule: u64) -> Vec<u8> {
    let mut page = Vec::with_capacity(27);
    page.extend_from_slice(b"OggS");
    page.push(0);
    page.push(0x04);
    page.extend_from_slice(&granule.to_le_bytes());
    page.extend_from_slice(&serial.to_le_bytes());
    page.extend_from_slice(&sequence.to_le_bytes());
    page.extend_from_slice(&0u32.to_le_bytes());
    page.push(0);
    let checksum = ogg_crc(&page);
    page[22..26].copy_from_slice(&checksum.to_le_bytes());
    page
}

fn ogg_crc(bytes: &[u8]) -> u32 {
    let mut crc = 0u32;
    for byte in bytes {
        crc ^= (*byte as u32) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn eos_page_has_valid_crc() {
        let mut page = eos_page(42, 7, 960);
        let expected = u32::from_le_bytes(page[22..26].try_into().unwrap());
        page[22..26].fill(0);
        assert_eq!(ogg_crc(&page), expected);
    }

    #[cfg(windows)]
    #[test]
    fn rename_retries_a_transient_windows_file_lock() {
        use std::os::windows::fs::OpenOptionsExt;

        let directory = std::env::temp_dir().join(format!(
            "nota-rename-retry-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("recording.ogg.copying");
        let destination = directory.join("recording.ogg");
        std::fs::write(&source, b"complete recording").unwrap();

        // Exclude FILE_SHARE_DELETE to reproduce a scanner temporarily
        // holding the freshly copied file on Windows.
        let held_file = OpenOptions::new()
            .read(true)
            .share_mode(0x0000_0001 | 0x0000_0002)
            .open(&source)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            drop(held_file);
        });

        rename_with_retry(&source, &destination).unwrap();
        release.join().unwrap();
        assert_eq!(std::fs::read(&destination).unwrap(), b"complete recording");
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_dir(directory).unwrap();
    }

    #[test]
    #[ignore = "requires NOTA_TEST_OUTPUT_DIRECTORY on another writable volume"]
    fn cross_volume_move_smoke_test() {
        let output_root = std::env::var_os("NOTA_TEST_OUTPUT_DIRECTORY")
            .map(PathBuf::from)
            .expect("set NOTA_TEST_OUTPUT_DIRECTORY to a writable test directory");
        let unique = format!(
            "nota-cross-volume-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let source_directory = std::env::temp_dir().join(&unique);
        let destination_directory = output_root.join(&unique);
        std::fs::create_dir_all(&source_directory).unwrap();
        std::fs::create_dir_all(&destination_directory).unwrap();
        let source = source_directory.join("session.partial.ogg");
        let destination = destination_directory.join("recording.ogg");
        let expected = b"verified cross-volume recording";
        std::fs::write(&source, expected).unwrap();

        move_verified(&source, &destination).unwrap();

        assert!(!source.exists());
        assert!(!destination.with_extension("ogg.copying").exists());
        assert_eq!(std::fs::read(&destination).unwrap(), expected);
        std::fs::remove_file(destination).unwrap();
        std::fs::remove_dir(destination_directory).unwrap();
        std::fs::remove_dir(source_directory).unwrap();
    }
}
