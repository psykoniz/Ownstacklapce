use serde_json::{Value, json};
use std::io::{Read, Write};

fn execute() -> Result<Value, String> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .map_err(|e| format!("failed to read stdin: {e}"))?;

    let payload = serde_json::from_str::<Value>(&input).unwrap_or_else(|_| json!({}));
    let name = payload
        .get("args")
        .and_then(|args| args.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("World");

    Ok(json!({
        "success": true,
        "output": format!("Hello, {}!", name)
    }))
}

#[unsafe(no_mangle)]
pub extern "C" fn run() -> i32 {
    let output = match execute() {
        Ok(v) => v,
        Err(err) => json!({
            "success": false,
            "output": "",
            "error": err
        }),
    };

    let serialized = match serde_json::to_string(&output) {
        Ok(s) => s,
        Err(err) => {
            let fallback = format!(
                "{{\"success\":false,\"output\":\"\",\"error\":\"serialization failed: {err}\"}}"
            );
            if std::io::stdout().write_all(fallback.as_bytes()).is_err() {
                return 1;
            }
            return 1;
        }
    };

    if std::io::stdout().write_all(serialized.as_bytes()).is_err() {
        return 1;
    }
    0
}

fn main() {
    let _ = run();
}
