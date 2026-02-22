use std::path::Path;

#[cfg(target_os = "windows")]
use std::fs::File;
#[cfg(target_os = "windows")]
use std::io::Error;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::RECT;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject,
    GetDIBits, GetWindowDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    GetDesktopWindow, GetForegroundWindow, GetWindowRect,
};

#[cfg(target_os = "windows")]
fn os_error(prefix: &str) -> String {
    format!("{}: {}", prefix, Error::last_os_error())
}

#[cfg(target_os = "windows")]
fn checked_pixel_len(width: i32, height: i32) -> Result<usize, String> {
    let width =
        usize::try_from(width).map_err(|_| "Invalid bitmap width".to_string())?;
    let height =
        usize::try_from(height).map_err(|_| "Invalid bitmap height".to_string())?;

    width
        .checked_mul(height)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| "Bitmap size overflow".to_string())
}

#[cfg(target_os = "windows")]
pub fn capture_active_window(output_path: &Path) -> Result<(), String> {
    let mut hwnd = unsafe {
        // SAFETY: Pure Win32 query, no pointers involved.
        GetForegroundWindow()
    };
    if hwnd == 0 {
        hwnd = unsafe {
            // SAFETY: Pure Win32 query, no pointers involved.
            GetDesktopWindow()
        };
    }
    if hwnd == 0 {
        return Err("Failed to resolve target window handle".to_string());
    }

    let mut rect: RECT = unsafe {
        // SAFETY: RECT is plain old data; zeroed is valid initialization.
        std::mem::zeroed()
    };
    let got_rect = unsafe {
        // SAFETY: `rect` points to valid writable memory for Win32 to fill.
        GetWindowRect(hwnd, &mut rect)
    };
    if got_rect == 0 {
        return Err(os_error("GetWindowRect failed"));
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err("Target window has invalid dimensions".to_string());
    }

    let mut pixels = unsafe {
        // SAFETY: All Win32 calls use validated handles. Resources are always
        // released before returning by explicit cleanup in this scope.
        let screen_dc = GetWindowDC(hwnd);
        if screen_dc == 0 {
            return Err(os_error("GetWindowDC failed"));
        }

        let memory_dc = CreateCompatibleDC(screen_dc);
        if memory_dc == 0 {
            let _ = ReleaseDC(hwnd, screen_dc);
            return Err(os_error("CreateCompatibleDC failed"));
        }

        let bitmap = CreateCompatibleBitmap(screen_dc, width, height);
        if bitmap == 0 {
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(hwnd, screen_dc);
            return Err(os_error("CreateCompatibleBitmap failed"));
        }

        let old_obj = SelectObject(memory_dc, bitmap as _);
        if old_obj == 0 || old_obj == -1 {
            let _ = DeleteObject(bitmap as _);
            let _ = DeleteDC(memory_dc);
            let _ = ReleaseDC(hwnd, screen_dc);
            return Err(os_error("SelectObject failed"));
        }

        let capture_result = (|| -> Result<Vec<u8>, String> {
            let copied = BitBlt(
                memory_dc,
                0,
                0,
                width,
                height,
                screen_dc,
                0,
                0,
                SRCCOPY | CAPTUREBLT,
            );
            if copied == 0 {
                return Err(os_error("BitBlt failed"));
            }

            let mut bitmap_info: BITMAPINFO = std::mem::zeroed();
            bitmap_info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // Top-down image.
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                ..std::mem::zeroed()
            };

            let mut data = vec![0u8; checked_pixel_len(width, height)?];
            let scanned = GetDIBits(
                memory_dc,
                bitmap,
                0,
                height as u32,
                data.as_mut_ptr().cast(),
                &mut bitmap_info,
                DIB_RGB_COLORS,
            );
            if scanned == 0 {
                return Err(os_error("GetDIBits failed"));
            }

            Ok(data)
        })();

        let _ = SelectObject(memory_dc, old_obj);
        let _ = DeleteObject(bitmap as _);
        let _ = DeleteDC(memory_dc);
        let _ = ReleaseDC(hwnd, screen_dc);

        capture_result?
    };

    // Convert BGRA (GDI) to RGBA for PNG encoding.
    for chunk in pixels.chunks_exact_mut(4) {
        chunk.swap(0, 2);
    }

    let file = File::create(output_path)
        .map_err(|e| format!("Failed to create screenshot file: {}", e))?;
    let mut encoder = png::Encoder::new(file, width as u32, height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder
        .write_header()
        .map_err(|e| format!("Failed to write PNG header: {}", e))?;
    writer
        .write_image_data(&pixels)
        .map_err(|e| format!("Failed to write PNG data: {}", e))?;

    Ok(())
}

#[cfg(all(test, target_os = "windows"))]
mod windows_tests {
    use super::capture_active_window;
    use std::io::Read;

    #[test]
    fn capture_writes_png_signature() {
        let temp = tempfile::tempdir().expect("tempdir");
        let output = temp.path().join("ui_screenshot.png");
        capture_active_window(&output).expect("capture");

        let mut f = std::fs::File::open(output).expect("open screenshot");
        let mut magic = [0u8; 8];
        f.read_exact(&mut magic).expect("read png signature");
        assert_eq!(magic, [137, 80, 78, 71, 13, 10, 26, 10]);
    }
}

#[cfg(not(target_os = "windows"))]
pub fn capture_active_window(_output_path: &Path) -> Result<(), String> {
    Err("Screenshot capture is currently supported on Windows only".to_string())
}

#[cfg(all(test, not(target_os = "windows")))]
mod non_windows_tests {
    use super::capture_active_window;

    #[test]
    fn capture_is_explicitly_unsupported() {
        let err = capture_active_window(std::path::Path::new("out.png"))
            .expect_err("non-windows capture should fail");
        assert!(err.contains("Windows only"));
    }
}
