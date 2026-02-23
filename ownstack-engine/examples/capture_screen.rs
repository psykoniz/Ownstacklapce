use std::path::Path;

fn main() {
    let output_path = Path::new("ownstack_window.png");

    #[cfg(target_os = "windows")]
    {
        // Try Lapce/OwnStack window titles in order
        let titles = ["Lapce", "OwnStack", "lapce"];
        let mut captured = false;

        for title in &titles {
            println!("Searching for window with title containing '{}'...", title);
            match ownstack_engine::vision::capture_window_by_title(
                title,
                output_path,
            ) {
                Ok(()) => {
                    let size =
                        std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
                    println!(
                        "✅ Window '{}' captured to: {}",
                        title,
                        output_path.display()
                    );
                    println!("   File size: {} KB ({} bytes)", size / 1024, size);
                    captured = true;
                    break;
                }
                Err(e) => {
                    println!("  ↳ Not found: {}", e);
                }
            }
        }

        if !captured {
            println!(
                "\nNo IDE window found — capturing full desktop as fallback..."
            );
            match ownstack_engine::vision::capture_screen(output_path) {
                Ok(()) => {
                    let size =
                        std::fs::metadata(output_path).map(|m| m.len()).unwrap_or(0);
                    println!("✅ Desktop captured: {} KB", size / 1024);
                }
                Err(e) => eprintln!("❌ Desktop capture failed: {}", e),
            }
        }
    }

    #[cfg(not(target_os = "windows"))]
    eprintln!("❌ Screenshot capture only supported on Windows");
}
