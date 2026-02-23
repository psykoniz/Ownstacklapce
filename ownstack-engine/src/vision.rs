use std::path::Path;

#[cfg(target_os = "windows")]
use std::fs::File;
#[cfg(target_os = "windows")]
use std::io::Error;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Foundation::RECT;
#[cfg(target_os = "windows")]
use windows_sys::Win32::Graphics::Gdi::{
    CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits,
    GetWindowDC, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    DIB_RGB_COLORS,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT,
};
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetDesktopWindow, GetForegroundWindow, GetSystemMetrics,
    GetWindowRect, GetWindowTextW, SM_CXSCREEN, SM_CYSCREEN,
};

// `PrintWindow` is not exposed through the `Win32_UI_WindowsAndMessaging`
// feature in windows-sys 0.52, so we declare it manually via raw FFI.
// SAFETY: The signature matches the Windows SDK docs for PrintWindow.
#[cfg(target_os = "windows")]
extern "system" {
    fn PrintWindow(hwnd: isize, hdcblt: isize, nflags: u32) -> i32;
}
#[cfg(target_os = "windows")]
use windows_sys::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
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
fn ensure_dpi_aware() {
    unsafe {
        // SAFETY: Pure Win32 call to set process state.
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

#[cfg(target_os = "windows")]
pub fn capture_active_window(output_path: &Path) -> Result<(), String> {
    ensure_dpi_aware();
    let mut hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        hwnd = unsafe { GetDesktopWindow() };
    }
    if hwnd == 0 {
        return Err("Failed to resolve target window handle".to_string());
    }

    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(hwnd, &mut rect) } == 0 {
        return Err(os_error("GetWindowRect failed"));
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;
    if width <= 0 || height <= 0 {
        return Err("Target window has invalid dimensions".to_string());
    }

    capture_window_to_file(hwnd, width, height, output_path)
}

#[cfg(target_os = "windows")]
pub fn capture_screen(output_path: &Path) -> Result<(), String> {
    ensure_dpi_aware();
    let hwnd = unsafe { GetDesktopWindow() };
    if hwnd == 0 {
        return Err("Failed to resolve desktop window handle".to_string());
    }

    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    if width <= 0 || height <= 0 {
        return Err("System reported invalid screen dimensions".to_string());
    }

    capture_window_to_file(hwnd, width, height, output_path)
}

#[cfg(target_os = "windows")]
fn capture_window_to_file(
    hwnd: isize,
    width: i32,
    height: i32,
    output_path: &Path,
) -> Result<(), String> {
    let mut pixels = unsafe {
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
        let capture_result = (|| -> Result<Vec<u8>, String> {
            // PrintWindow with PW_RENDERFULLCONTENT forces GPU-composited
            // content (wgpu / Vulkan / etc.) to be rendered into the DC.
            // This is the only reliable way to capture hardware-accelerated
            // windows like Lapce without getting a blank frame.
            const PW_RENDERFULLCONTENT: u32 = 0x00000002;
            let printed = PrintWindow(hwnd, memory_dc, PW_RENDERFULLCONTENT);
            if printed == 0 {
                return Err(os_error("PrintWindow failed"));
            }

            let mut bitmap_info: BITMAPINFO = std::mem::zeroed();
            bitmap_info.bmiHeader = BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
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

#[cfg(target_os = "windows")]
pub fn capture_window_by_title(
    title_substring: &str,
    output_path: &Path,
) -> Result<(), String> {
    ensure_dpi_aware();
    let target_hwnd: isize;

    unsafe {
        struct Search {
            substring: String,
            found_hwnd: isize,
        }

        let mut search = Search {
            substring: title_substring.to_string(),
            found_hwnd: 0,
        };

        unsafe extern "system" fn cb(hwnd: isize, lparam: isize) -> i32 {
            let search = &mut *(lparam as *mut Search);
            let mut buffer = [0u16; 512];
            let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), 512);
            if len > 0 {
                let title = String::from_utf16_lossy(&buffer[..len as usize]);
                if title
                    .to_lowercase()
                    .contains(&search.substring.to_lowercase())
                {
                    search.found_hwnd = hwnd;
                    return 0; // Stop
                }
            }
            1
        }

        EnumWindows(Some(cb), &mut search as *mut _ as isize);
        target_hwnd = search.found_hwnd;
    }

    if target_hwnd == 0 {
        return Err(format!("Window containing '{}' not found", title_substring));
    }

    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { GetWindowRect(target_hwnd, &mut rect) } == 0 {
        return Err(os_error("GetWindowRect failed"));
    }

    let width = rect.right - rect.left;
    let height = rect.bottom - rect.top;

    if width <= 0 || height <= 0 {
        return Err("Target window has invalid dimensions".to_string());
    }

    capture_window_to_file(target_hwnd, width, height, output_path)
}

#[cfg(target_os = "windows")]
pub fn mouse_click(x: i32, y: i32) -> Result<(), String> {
    ensure_dpi_aware();
    unsafe {
        let screen_width = GetSystemMetrics(SM_CXSCREEN);
        let screen_height = GetSystemMetrics(SM_CYSCREEN);

        if screen_width == 0 || screen_height == 0 {
            return Err("Failed to get screen metrics".to_string());
        }

        // Convert coordinates to absolute (0-65535) as required by MOUSEEVENTF_ABSOLUTE
        let abs_x = (x * 65536) / screen_width;
        let abs_y = (y * 65536) / screen_height;

        let mut inputs: [INPUT; 3] = std::mem::zeroed();

        // 1. Move to position
        inputs[0].r#type = INPUT_MOUSE;
        inputs[0].Anonymous.mi = MOUSEINPUT {
            dx: abs_x,
            dy: abs_y,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE,
            time: 0,
            dwExtraInfo: 0,
        };

        // 2. Left Down
        inputs[1].r#type = INPUT_MOUSE;
        inputs[1].Anonymous.mi = MOUSEINPUT {
            dx: abs_x,
            dy: abs_y,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_LEFTDOWN | MOUSEEVENTF_ABSOLUTE,
            time: 0,
            dwExtraInfo: 0,
        };

        // 3. Left Up
        inputs[2].r#type = INPUT_MOUSE;
        inputs[2].Anonymous.mi = MOUSEINPUT {
            dx: abs_x,
            dy: abs_y,
            mouseData: 0,
            dwFlags: MOUSEEVENTF_LEFTUP | MOUSEEVENTF_ABSOLUTE,
            time: 0,
            dwExtraInfo: 0,
        };

        let sent = SendInput(
            inputs.len() as u32,
            inputs.as_ptr(),
            std::mem::size_of::<INPUT>() as i32,
        );

        if sent != inputs.len() as u32 {
            return Err(os_error("SendInput failed"));
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
pub fn click_active_window(rel_x: i32, rel_y: i32) -> Result<(), String> {
    ensure_dpi_aware();
    let mut hwnd = unsafe { GetForegroundWindow() };
    if hwnd == 0 {
        hwnd = unsafe { GetDesktopWindow() };
    }
    if hwnd == 0 {
        return Err("Failed to resolve target window handle".to_string());
    }

    let mut rect: RECT = unsafe { std::mem::zeroed() };
    let got_rect = unsafe { GetWindowRect(hwnd, &mut rect) };
    if got_rect == 0 {
        return Err(os_error("GetWindowRect failed"));
    }

    let abs_x = rect.left + rel_x;
    let abs_y = rect.top + rel_y;

    mouse_click(abs_x, abs_y)
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

#[cfg(not(target_os = "windows"))]
pub fn capture_screen(_output_path: &Path) -> Result<(), String> {
    Err("Screenshot capture is currently supported on Windows only".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn capture_window_by_title(
    _title_substring: &str,
    _output_path: &Path,
) -> Result<(), String> {
    Err("Screenshot capture is currently supported on Windows only".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn mouse_click(_x: i32, _y: i32) -> Result<(), String> {
    Err("Mouse interaction is currently supported on Windows only".to_string())
}

#[cfg(not(target_os = "windows"))]
pub fn click_active_window(_rel_x: i32, _rel_y: i32) -> Result<(), String> {
    Err("Mouse interaction is currently supported on Windows only".to_string())
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
