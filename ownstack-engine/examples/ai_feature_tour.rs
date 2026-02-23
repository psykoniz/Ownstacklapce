use std::path::Path;
use std::thread::sleep;
use std::time::Duration;

#[cfg(target_os = "windows")]
fn main() {
    use ownstack_engine::vision::{capture_window_by_title, mouse_click};
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextW,
        SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    println!("Searching for OwnStack IDE window...");

    // Find handle for the OwnStack / Lapce window
    let hwnd = unsafe { find_window_by_title("Lapce") }
        .or_else(|| unsafe { find_window_by_title("OwnStack") });

    let Some(hwnd) = hwnd else {
        eprintln!("OwnStack IDE not found — launching it now...");
        std::process::Command::new(r"target\release\ownstack-ide.exe")
            .spawn()
            .expect("failed to launch ownstack-ide.exe");
        sleep(Duration::from_secs(4));
        return main(); // retry
    };

    println!("✅ Found window handle: {}", hwnd);

    // Bring window to foreground and restore if minimised
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
    sleep(Duration::from_millis(600));

    // ── Get window geometry ──────────────────────────────────────────────────
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    unsafe { GetWindowRect(hwnd, &mut rect) };
    let win_x = rect.left;
    let win_y = rect.top;
    let win_w = rect.right - rect.left;
    let win_h = rect.bottom - rect.top;
    println!("Window: {}x{} at ({}, {})", win_w, win_h, win_x, win_y);

    // Helper closure: capture + save
    let capture = |name: &str| {
        let p = format!("{}.png", name);
        let path = Path::new(&p);
        match capture_window_by_title("Lapce", path)
            .or_else(|_| capture_window_by_title("OwnStack", path))
        {
            Ok(()) => {
                let sz = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                println!("📸 {} — {}KB", name, sz / 1024);
            }
            Err(e) => eprintln!("❌ Capture failed: {}", e),
        }
    };

    // Helper closure: click relative to window
    let click = |rel_x: f32, rel_y: f32, label: &str| {
        let abs_x = win_x + (win_w as f32 * rel_x) as i32;
        let abs_y = win_y + (win_h as f32 * rel_y) as i32;
        println!("🖱️  Clicking '{}' at abs ({}, {})", label, abs_x, abs_y);
        if let Err(e) = mouse_click(abs_x, abs_y) {
            eprintln!("  ↳ click error: {}", e);
        }
        sleep(Duration::from_millis(800));
    };

    // ── Step 0: Initial capture ──────────────────────────────────────────────
    println!("\n── Step 0: Initial state ──");
    capture("step0_launch");

    // ── Step 1: Dismiss the Welcome/Setup modal (click "Skip") ────────────────
    // "Skip" button is roughly at 42% width, 57% height of the window
    println!("\n── Step 1: Skip welcome dialog ──");
    click(0.42, 0.57, "Skip button");
    sleep(Duration::from_millis(500));
    capture("step1_after_skip");

    // ── Step 2: Click the "OwnStack AI Chat" right panel header ──────────────
    println!("\n── Step 2: Open AI Chat panel ──");
    // AI Chat panel header is at ~88% width, ~10% height
    click(0.88, 0.10, "OwnStack AI Chat header");
    sleep(Duration::from_millis(600));
    capture("step2_ai_chat_panel");

    // ── Step 3: Click the ASK button in the status bar (bottom bar, ~9% x, 97% y)
    println!("\n── Step 3: Click ASK mode button ──");
    click(0.09, 0.97, "ASK mode button");
    sleep(Duration::from_millis(600));
    capture("step3_ask_mode");

    // ── Step 4: Click AI input field and type a message ─────────────────────
    println!("\n── Step 4: Focus AI input ──");
    // Bottom-right input "Ask OwnStack anything" at ~72% x, 94% y
    click(0.72, 0.94, "AI chat input");
    sleep(Duration::from_millis(400));

    // Type using Windows keyboard events
    type_text("Explain the OwnStack policy engine");
    sleep(Duration::from_millis(600));
    capture("step4_ai_input_typed");

    println!("\n✅ AI feature tour complete.");
    println!(
        "   Screenshots saved: step0_launch.png through step4_ai_input_typed.png"
    );
}

#[cfg(target_os = "windows")]
unsafe fn find_window_by_title(substring: &str) -> Option<isize> {
    use windows_sys::Win32::System::ProcessStatus::K32GetProcessImageFileNameW;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_INFORMATION,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
    };

    struct Search {
        needle: String,
        result: isize,
    }

    let mut s = Search {
        needle: substring.to_lowercase(),
        result: 0,
    };

    unsafe extern "system" fn cb(hwnd: isize, lparam: isize) -> i32 {
        use windows_sys::Win32::System::ProcessStatus::K32GetProcessImageFileNameW;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_INFORMATION,
        };
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        };

        let s = &mut *(lparam as *mut Search);
        if IsWindowVisible(hwnd) == 0 {
            return 1;
        }

        // Get the process name owning this window
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, &mut pid);
        let hproc = OpenProcess(PROCESS_QUERY_INFORMATION, 0, pid);
        if hproc == 0 {
            return 1;
        }
        let mut name_buf = [0u16; 512];
        let name_len =
            K32GetProcessImageFileNameW(hproc, name_buf.as_mut_ptr(), 512);
        windows_sys::Win32::Foundation::CloseHandle(hproc);

        if name_len > 0 {
            let proc_name = String::from_utf16_lossy(&name_buf[..name_len as usize])
                .to_lowercase();
            // Match only if the process is ownstack-ide or lapce (not code.exe / vscode)
            let is_ownstack = proc_name.contains("ownstack-ide")
                || proc_name.ends_with("lapce.exe");
            if !is_ownstack {
                return 1;
            }
        } else {
            return 1;
        }

        // Also require the window to have a non-empty title to avoid invisible child windows
        let mut title_buf = [0u16; 512];
        let title_len = GetWindowTextW(hwnd, title_buf.as_mut_ptr(), 512);
        if title_len == 0 {
            return 1;
        }

        s.result = hwnd;
        0 // Stop enumeration — first visible OwnStack window wins
    }

    EnumWindows(Some(cb), &mut s as *mut _ as isize);
    if s.result != 0 {
        Some(s.result)
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
fn type_text(text: &str) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_UNICODE,
    };
    for ch in text.encode_utf16() {
        let mut inputs = [INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: unsafe { std::mem::zeroed() },
        }; 2];
        // key down
        inputs[0].Anonymous.ki = KEYBDINPUT {
            wVk: 0,
            wScan: ch,
            dwFlags: KEYEVENTF_UNICODE,
            time: 0,
            dwExtraInfo: 0,
        };
        // key up
        inputs[1].Anonymous.ki = KEYBDINPUT {
            wVk: 0,
            wScan: ch,
            dwFlags: KEYEVENTF_UNICODE | 0x0002, // KEYEVENTF_KEYUP
            time: 0,
            dwExtraInfo: 0,
        };
        unsafe {
            SendInput(2, inputs.as_ptr(), std::mem::size_of::<INPUT>() as i32);
        }
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("AI feature tour is Windows-only");
}
