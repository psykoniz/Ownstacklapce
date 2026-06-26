//! Probe the exec sandbox directly on the current OS to see which command
//! forms work (direct exe vs shell builtins/redirects).
use ownstack_agent::toolkits::{CoreToolkit, Toolkit};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let ws = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));
    std::fs::create_dir_all(&ws).ok();
    let tk = CoreToolkit::new(ws.clone(), "probe".to_string(), None);

    let cmds = [
        "python --version",
        "cargo --version",
        "git --version",
        "echo hello",                       // Windows builtin (no echo.exe)
        "cmd /c echo hello",                // explicit cmd wrapper
        "echo DONE > made.txt",             // redirect (shell feature)
        "cmd /c echo DONE > made2.txt",     // redirect via cmd
    ];

    for c in cmds {
        let res = tk.execute("exec", serde_json::json!({"command": c})).await;
        match res {
            Ok(r) => {
                let so: String = r.stdout.chars().take(90).collect();
                let se: String = r.stderr.chars().take(90).collect();
                println!("[{}] success={} exit={:?} stdout={:?} stderr={:?}",
                    c, r.success, r.exit_code, so.replace('\n', " "), se.replace('\n', " "));
            }
            Err(e) => println!("[{}] TOOL_ERR {:?}", c, e),
        }
    }
    println!("--- files ---");
    for e in std::fs::read_dir(&ws).unwrap().flatten() {
        println!("  {}", e.file_name().to_string_lossy());
    }
}
