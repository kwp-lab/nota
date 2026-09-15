use crate::models::CaptureTarget;
use anyhow::{Result, anyhow};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use std::collections::HashMap;
use std::ffi::c_void;
use std::fs;
use std::mem::size_of;
use std::slice;
use std::sync::{Mutex, OnceLock};
use std::time::SystemTime;
use windows::Win32::Foundation::{HWND, LPARAM, RPC_E_CHANGED_MODE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, HBITMAP, HDC, HGDIOBJ, SelectObject,
};
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::UI::Shell::{SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON, SHGetFileInfoW};
use windows::Win32::UI::WindowsAndMessaging::{
    CopyIcon, DI_NORMAL, DestroyIcon, DrawIconEx, GCLP_HICON, GCLP_HICONSM, GetClassLongPtrW,
    HICON, ICON_BIG, ICON_SMALL, ICON_SMALL2, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_GETICON,
};
use windows::core::PCWSTR;

const ICON_SIZE: u32 = 32;
const WINDOW_ICON_TIMEOUT_MS: u32 = 100;

#[derive(Clone)]
struct CachedIcon {
    modified_at: Option<SystemTime>,
    data_url: String,
}

static ICON_CACHE: OnceLock<Mutex<HashMap<String, CachedIcon>>> = OnceLock::new();

pub fn attach_capture_target_icons(targets: &mut [CaptureTarget]) {
    let _com = ComApartment::initialize();
    for target in targets {
        target.icon_data_url = icon_data_url(target);
    }
}

fn icon_data_url(target: &CaptureTarget) -> Option<String> {
    let cache_key = normalize_cache_key(&target.executable_path);
    let modified_at = fs::metadata(&target.executable_path)
        .and_then(|metadata| metadata.modified())
        .ok();

    if !cache_key.is_empty()
        && let Some(cached) = icon_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .filter(|cached| cached.modified_at == modified_at)
    {
        return Some(cached.data_url.clone());
    }

    let icon = shell_icon(&target.executable_path).or_else(|| window_icon(target.window_handle));
    let data_url = icon
        .as_ref()
        .and_then(|icon| render_icon_rgba(icon.0).ok())
        .and_then(|rgba| encode_png_data_url(ICON_SIZE, ICON_SIZE, &rgba).ok())?;

    if !cache_key.is_empty() {
        icon_cache()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                cache_key,
                CachedIcon {
                    modified_at,
                    data_url: data_url.clone(),
                },
            );
    }
    Some(data_url)
}

fn icon_cache() -> &'static Mutex<HashMap<String, CachedIcon>> {
    ICON_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn normalize_cache_key(path: &str) -> String {
    path.replace('/', "\\").to_lowercase()
}

fn shell_icon(path: &str) -> Option<OwnedIcon> {
    if path.is_empty() {
        return None;
    }
    let wide_path = path.encode_utf16().chain(Some(0)).collect::<Vec<_>>();
    let mut info = SHFILEINFOW::default();
    let result = unsafe {
        SHGetFileInfoW(
            PCWSTR(wide_path.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        )
    };
    if result == 0 || info.hIcon.0.is_null() {
        return None;
    }
    Some(OwnedIcon(info.hIcon))
}

fn window_icon(window_handle: Option<isize>) -> Option<OwnedIcon> {
    let hwnd = HWND(window_handle? as *mut c_void);
    for kind in [ICON_BIG, ICON_SMALL2, ICON_SMALL] {
        let mut icon_result = 0usize;
        let message_result = unsafe {
            SendMessageTimeoutW(
                hwnd,
                WM_GETICON,
                WPARAM(kind as usize),
                LPARAM(96),
                SMTO_ABORTIFHUNG,
                WINDOW_ICON_TIMEOUT_MS,
                Some(&mut icon_result),
            )
        };
        if message_result.0 != 0
            && icon_result != 0
            && let Ok(icon) = unsafe { CopyIcon(HICON(icon_result as *mut c_void)) }
        {
            return Some(OwnedIcon(icon));
        }
    }

    for class_index in [GCLP_HICON, GCLP_HICONSM] {
        let icon_result = unsafe { GetClassLongPtrW(hwnd, class_index) };
        if icon_result != 0
            && let Ok(icon) = unsafe { CopyIcon(HICON(icon_result as *mut c_void)) }
        {
            return Some(OwnedIcon(icon));
        }
    }
    None
}

fn render_icon_rgba(icon: HICON) -> Result<Vec<u8>> {
    let black = draw_icon_on_background(icon, 0)?;
    let white = draw_icon_on_background(icon, 255)?;
    let mut rgba = Vec::with_capacity((ICON_SIZE * ICON_SIZE * 4) as usize);
    for (black_pixel, white_pixel) in black
        .as_chunks::<4>()
        .0
        .iter()
        .zip(white.as_chunks::<4>().0)
    {
        let alpha = (0..3)
            .map(|channel| {
                255u8.saturating_sub(white_pixel[channel].saturating_sub(black_pixel[channel]))
            })
            .min()
            .unwrap_or(0);
        for channel in [2usize, 1, 0] {
            let value = if alpha == 0 {
                0
            } else {
                ((u16::from(black_pixel[channel]) * 255 + u16::from(alpha) / 2) / u16::from(alpha))
                    .min(255) as u8
            };
            rgba.push(value);
        }
        rgba.push(alpha);
    }
    Ok(rgba)
}

fn draw_icon_on_background(icon: HICON, background: u8) -> Result<Vec<u8>> {
    let dc = OwnedDc::new()?;
    let mut bitmap_info = BITMAPINFO::default();
    bitmap_info.bmiHeader.biSize =
        size_of::<windows::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
    bitmap_info.bmiHeader.biWidth = ICON_SIZE as i32;
    bitmap_info.bmiHeader.biHeight = -(ICON_SIZE as i32);
    bitmap_info.bmiHeader.biPlanes = 1;
    bitmap_info.bmiHeader.biBitCount = 32;
    bitmap_info.bmiHeader.biCompression = BI_RGB.0;

    let mut bits = std::ptr::null_mut();
    let bitmap =
        unsafe { CreateDIBSection(Some(dc.0), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0) }
            .map(OwnedBitmap)?;
    if bits.is_null() {
        return Err(anyhow!("Windows did not return icon bitmap pixels"));
    }

    let previous = unsafe { SelectObject(dc.0, HGDIOBJ(bitmap.0.0)) };
    if previous.0.is_null() {
        return Err(anyhow!("Windows could not select the icon bitmap"));
    }

    let byte_len = (ICON_SIZE * ICON_SIZE * 4) as usize;
    let result = (|| {
        let pixels = unsafe { slice::from_raw_parts_mut(bits.cast::<u8>(), byte_len) };
        for pixel in pixels.as_chunks_mut::<4>().0 {
            pixel.fill(background);
        }
        unsafe {
            DrawIconEx(
                dc.0,
                0,
                0,
                icon,
                ICON_SIZE as i32,
                ICON_SIZE as i32,
                0,
                None,
                DI_NORMAL,
            )?;
        }
        Ok::<_, anyhow::Error>(pixels.to_vec())
    })();
    unsafe {
        SelectObject(dc.0, previous);
    }
    result
}

fn encode_png_data_url(width: u32, height: u32, rgba: &[u8]) -> Result<String> {
    if rgba.len() != (width * height * 4) as usize {
        return Err(anyhow!("invalid RGBA icon buffer length"));
    }
    let mut encoded = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut encoded, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(rgba)?;
    }
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(encoded)
    ))
}

struct ComApartment {
    uninitialize: bool,
}

impl ComApartment {
    fn initialize() -> Self {
        let result = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        Self {
            uninitialize: result.is_ok() && result != RPC_E_CHANGED_MODE,
        }
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        if self.uninitialize {
            unsafe { CoUninitialize() };
        }
    }
}

struct OwnedIcon(HICON);

impl Drop for OwnedIcon {
    fn drop(&mut self) {
        let _ = unsafe { DestroyIcon(self.0) };
    }
}

struct OwnedDc(HDC);

impl OwnedDc {
    fn new() -> Result<Self> {
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.0.is_null() {
            return Err(anyhow!("Windows could not create an icon device context"));
        }
        Ok(Self(dc))
    }
}

impl Drop for OwnedDc {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteDC(self.0);
        }
    }
}

struct OwnedBitmap(HBITMAP);

impl Drop for OwnedBitmap {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(self.0.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_windows_icon_cache_keys() {
        assert_eq!(
            normalize_cache_key("C:/Program Files/Meeting/App.EXE"),
            "c:\\program files\\meeting\\app.exe"
        );
    }

    #[test]
    fn encodes_rgba_as_png_data_url() {
        let rgba = vec![255u8; (ICON_SIZE * ICON_SIZE * 4) as usize];
        let data_url = encode_png_data_url(ICON_SIZE, ICON_SIZE, &rgba).unwrap();
        let encoded = data_url.strip_prefix("data:image/png;base64,").unwrap();
        let decoded = STANDARD.decode(encoded).unwrap();
        assert_eq!(&decoded[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn missing_icon_source_degrades_without_error() {
        let target = CaptureTarget {
            id: "process:1".into(),
            kind: "process".into(),
            display_name: "Missing".into(),
            process_id: 1,
            executable_path: r"Z:\nota-does-not-exist\missing.exe".into(),
            icon_data_url: None,
            window_handle: None,
            browser: false,
            priority: 0,
        };
        assert!(icon_data_url(&target).is_none());
    }

    #[test]
    fn extracts_and_renders_the_current_windows_executable_icon() {
        let _com = ComApartment::initialize();
        let executable = std::env::current_exe().unwrap();
        let icon = shell_icon(executable.to_string_lossy().as_ref()).unwrap();
        let rgba = render_icon_rgba(icon.0).unwrap();
        assert_eq!(rgba.len(), (ICON_SIZE * ICON_SIZE * 4) as usize);
        assert!(rgba.as_chunks::<4>().0.iter().any(|pixel| pixel[3] > 0));
    }
}
