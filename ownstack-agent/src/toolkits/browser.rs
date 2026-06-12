//! Browser Automation Toolkit — Headless Chrome via Chrome DevTools Protocol (CDP).
//!
//! Provides real browser automation using a headless Chromium instance.
//! Falls back to HTTP-based content fetching when no browser binary is available.

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// CDP connection
// ---------------------------------------------------------------------------

/// Locate a usable Chromium/Chrome binary on the system.
fn find_chrome_binary() -> Option<String> {
    let candidates = [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/snap/bin/chromium",
        // macOS
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
    ];
    for bin in &candidates {
        if Command::new(bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return Some(bin.to_string());
        }
    }
    None
}

/// Launch headless Chrome, returning (child, ws_url, port).
fn launch_headless_chrome(
    chrome_bin: &str,
) -> Result<(std::process::Child, String, u16), String> {
    // Find a free port
    let listener = std::net::TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("bind failed: {e}"))?;
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let mut child = Command::new(chrome_bin)
        .args([
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            "--disable-extensions",
            "--disable-background-networking",
            &format!("--remote-debugging-port={port}"),
            "about:blank",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Failed to launch Chrome: {e}"))?;

    // Wait for DevTools WebSocket URL from stderr
    let stderr = child.stderr.take().unwrap();
    let reader = BufReader::new(stderr);
    let mut ws_url = String::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(10);

    for line in reader.lines() {
        if std::time::Instant::now() > deadline {
            let _ = child.kill();
            return Err("Chrome startup timeout".to_string());
        }
        let line = line.map_err(|e| format!("stderr read: {e}"))?;
        debug!("Chrome stderr: {}", line);
        if line.contains("DevTools listening on ") {
            ws_url = line
                .split("DevTools listening on ")
                .nth(1)
                .unwrap_or("")
                .trim()
                .to_string();
            break;
        }
    }

    if ws_url.is_empty() {
        let _ = child.kill();
        return Err("Could not find DevTools WebSocket URL".to_string());
    }

    info!("Chrome headless started on port {port}, ws={ws_url}");
    Ok((child, ws_url, port))
}

// ---------------------------------------------------------------------------
// CDP JSON-RPC over WebSocket
// ---------------------------------------------------------------------------

/// Fetch the first page target's WebSocket debugger URL.
async fn cdp_page_ws_url(port: u16) -> Result<String, String> {
    let list_url = format!("http://127.0.0.1:{port}/json/list");
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;
    let pages: Vec<serde_json::Value> = client
        .get(&list_url)
        .send()
        .await
        .map_err(|e| format!("GET /json/list: {e}"))?
        .json()
        .await
        .map_err(|e| format!("parse /json/list: {e}"))?;
    pages
        .iter()
        .find(|p| p.get("type").and_then(|t| t.as_str()) == Some("page"))
        .or_else(|| pages.first())
        .and_then(|p| p["webSocketDebuggerUrl"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "no page target/webSocketDebuggerUrl".to_string())
}

/// A minimal Chrome DevTools Protocol client over a single page WebSocket.
struct CdpClient {
    ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    >,
    next_id: u64,
}

impl CdpClient {
    async fn connect(ws_url: &str) -> Result<Self, String> {
        let (ws, _resp) = tokio_tungstenite::connect_async(ws_url)
            .await
            .map_err(|e| format!("CDP connect: {e}"))?;
        Ok(Self { ws, next_id: 1 })
    }

    /// Send a CDP command and await the matching result.
    async fn call(
        &mut self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        use futures_util::{SinkExt, StreamExt};
        use tokio_tungstenite::tungstenite::Message;

        let id = self.next_id;
        self.next_id += 1;
        let req = json!({ "id": id, "method": method, "params": params });
        self.ws
            .send(Message::Text(req.to_string()))
            .await
            .map_err(|e| format!("CDP send {method}: {e}"))?;

        // Read messages until we see the response with our id (skip events).
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(format!("CDP timeout waiting for {method}"));
            }
            let msg = tokio::time::timeout(remaining, self.ws.next())
                .await
                .map_err(|_| format!("CDP timeout for {method}"))?
                .ok_or_else(|| "CDP socket closed".to_string())?
                .map_err(|e| format!("CDP recv: {e}"))?;
            let text = match msg {
                Message::Text(t) => t,
                Message::Close(_) => return Err("CDP socket closed".to_string()),
                _ => continue,
            };
            let v: serde_json::Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if v.get("id").and_then(|i| i.as_u64()) == Some(id) {
                if let Some(err) = v.get("error") {
                    return Err(format!("CDP error for {method}: {err}"));
                }
                return Ok(v.get("result").cloned().unwrap_or(json!({})));
            }
            // otherwise it's an event; keep reading
        }
    }
}

/// JS string escaping for embedding a selector/value in a Runtime.evaluate expr.
fn js_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'").replace('\n', "\\n")
}

/// Run a full interactive CDP session: navigate, optionally act, return a summary.
async fn cdp_interactive(
    port: u16,
    url: &str,
    action: &str,
    selector: Option<&str>,
    text: Option<&str>,
    wait_ms: u64,
) -> Result<String, String> {
    let ws_url = cdp_page_ws_url(port).await?;
    let mut cdp = CdpClient::connect(&ws_url).await?;

    cdp.call("Page.enable", json!({})).await.ok();
    cdp.call("Runtime.enable", json!({})).await.ok();
    cdp.call("Page.navigate", json!({ "url": url })).await?;
    tokio::time::sleep(Duration::from_millis(wait_ms)).await;

    match action {
        "click" => {
            let sel = selector.unwrap_or("body");
            let expr = format!(
                "(() => {{ const el = document.querySelector('{}'); if(!el) return 'NOT_FOUND'; el.click(); return 'CLICKED'; }})()",
                js_escape(sel)
            );
            let res = cdp
                .call(
                    "Runtime.evaluate",
                    json!({ "expression": expr, "returnByValue": true }),
                )
                .await?;
            let outcome = res["result"]["value"].as_str().unwrap_or("");
            if outcome == "NOT_FOUND" {
                Err(format!("selector '{sel}' not found on {url}"))
            } else {
                Ok(format!("Clicked '{sel}' on {url}"))
            }
        }
        "type" => {
            let sel = selector.unwrap_or("input");
            let txt = text.unwrap_or("");
            let expr = format!(
                "(() => {{ const el = document.querySelector('{}'); if(!el) return 'NOT_FOUND'; el.focus(); el.value = '{}'; el.dispatchEvent(new Event('input', {{bubbles:true}})); return 'TYPED'; }})()",
                js_escape(sel),
                js_escape(txt)
            );
            let res = cdp
                .call(
                    "Runtime.evaluate",
                    json!({ "expression": expr, "returnByValue": true }),
                )
                .await?;
            let outcome = res["result"]["value"].as_str().unwrap_or("");
            if outcome == "NOT_FOUND" {
                Err(format!("selector '{sel}' not found on {url}"))
            } else {
                Ok(format!("Typed into '{sel}' on {url}"))
            }
        }
        "eval" => {
            let expr = text.unwrap_or("document.title");
            let res = cdp
                .call(
                    "Runtime.evaluate",
                    json!({ "expression": expr, "returnByValue": true }),
                )
                .await?;
            Ok(format!(
                "eval result: {}",
                res["result"]["value"]
            ))
        }
        _ => Ok(format!("Navigated to {url}")),
    }
}


// ---------------------------------------------------------------------------
// Fallback: HTTP fetch (no Chrome needed)
// ---------------------------------------------------------------------------

fn http_fetch_page(url: &str) -> Result<(String, u16, HashMap<String, String>), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("OwnStack-Agent/0.1 (headless browser fallback)")
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .map_err(|e| format!("HTTP client: {e}"))?;

    let resp = client
        .get(url)
        .send()
        .map_err(|e| format!("HTTP request failed: {e}"))?;

    let status = resp.status().as_u16();
    let mut headers = HashMap::new();
    for (k, v) in resp.headers() {
        headers.insert(k.to_string(), v.to_str().unwrap_or("").to_string());
    }
    let _content_type = headers
        .get("content-type")
        .cloned()
        .unwrap_or_default();

    let body = resp
        .text()
        .map_err(|e| format!("Read body: {e}"))?;

    // Truncate extremely large pages
    let body = if body.len() > 100_000 {
        format!("{}... [truncated, {} bytes total]", &body[..100_000], body.len())
    } else {
        body
    };

    Ok((body, status, headers))
}

/// Extract readable text from HTML (simple tag stripping).
/// Note: the `regex` crate has no backreferences, so script/style are stripped
/// with separate patterns rather than a single `</\1>` capture.
fn extract_text(html: &str) -> String {
    // Remove script and style blocks separately
    let re_script = regex::Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let re_style = regex::Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let cleaned = re_script.replace_all(html, "");
    let cleaned = re_style.replace_all(&cleaned, "");
    // Remove HTML tags
    let re_tags = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re_tags.replace_all(&cleaned, " ");
    // Collapse whitespace
    let re_ws = regex::Regex::new(r"\s+").unwrap();
    let text = re_ws.replace_all(&text, " ");
    text.trim().to_string()
}

/// Extract all links from HTML.
fn extract_links(html: &str) -> Vec<(String, String)> {
    let re = regex::Regex::new(r#"<a\s[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#).unwrap();
    re.captures_iter(html)
        .take(50)
        .filter_map(|cap| {
            let href = cap.get(1)?.as_str().to_string();
            let text = extract_text(cap.get(2)?.as_str());
            Some((href, text))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// BrowserToolkit
// ---------------------------------------------------------------------------

/// Browser automation toolkit with headless Chrome support and HTTP fallback.
pub struct BrowserToolkit;

#[derive(Debug, Deserialize)]
struct BrowseArgs {
    url: String,
    #[serde(default = "default_action")]
    action: String,
    #[serde(default)]
    selector: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    extract: Option<String>,
    #[serde(default)]
    wait_ms: Option<u64>,
}

fn default_action() -> String {
    "navigate".to_string()
}

#[async_trait]
impl Toolkit for BrowserToolkit {
    fn name(&self) -> &str {
        "browser"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "browse_url".to_string(),
                description: "Navigate to a URL, fetch content, and extract information. \
                    Uses headless Chrome when available, falls back to HTTP fetch."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "The URL to visit"},
                        "action": {
                            "type": "string",
                            "enum": ["navigate", "click", "type", "eval", "screenshot", "extract_text", "extract_links"],
                            "default": "navigate",
                            "description": "Action to perform on the page"
                        },
                        "selector": {"type": "string", "description": "CSS selector for click/type actions"},
                        "text": {"type": "string", "description": "Text to type into a selected element"},
                        "extract": {
                            "type": "string",
                            "enum": ["text", "links", "html", "title"],
                            "description": "What to extract from the page"
                        },
                        "wait_ms": {"type": "integer", "description": "Milliseconds to wait after navigation", "default": 1000},
                    },
                    "required": ["url"],
                }),
            },
            ToolDef {
                name: "browser_screenshot".to_string(),
                description: "Take a screenshot of a web page using headless Chrome"
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "URL to capture"},
                        "output_path": {"type": "string", "description": "Where to save the screenshot (PNG)"},
                        "width": {"type": "integer", "default": 1280},
                        "height": {"type": "integer", "default": 720},
                    },
                    "required": ["url"],
                }),
            },
        ]
    }

    async fn execute(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        match tool_name {
            "browse_url" => execute_browse_url(args).await,
            "browser_screenshot" => execute_screenshot(args).await,
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

async fn execute_browse_url(args: serde_json::Value) -> Result<ToolResult, ToolkitError> {
    let parsed: BrowseArgs = serde_json::from_value(args)
        .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;

    info!("BrowserToolkit: browse_url action={} url={}", parsed.action, parsed.url);

    // Validate URL
    if !parsed.url.starts_with("http://") && !parsed.url.starts_with("https://") {
        return Err(ToolkitError::InvalidArguments(
            "URL must start with http:// or https://".to_string(),
        ));
    }

    let chrome_bin = find_chrome_binary();
    let interactive = matches!(parsed.action.as_str(), "click" | "type" | "eval");

    if interactive {
        let Some(chrome) = chrome_bin else {
            return Ok(ToolResult::failure(
                format!(
                    "Action '{}' requires Chrome/Chromium, which was not found. Install chromium or google-chrome.",
                    parsed.action
                ),
                Some(1),
            ));
        };
        // Launch Chrome and drive it over a real CDP WebSocket session.
        match launch_headless_chrome(&chrome) {
            Ok((mut child, _ws_url, port)) => {
                let wait = parsed.wait_ms.unwrap_or(1500);
                let result = cdp_interactive(
                    port,
                    &parsed.url,
                    &parsed.action,
                    parsed.selector.as_deref(),
                    parsed.text.as_deref(),
                    wait,
                )
                .await;
                let _ = child.kill();
                let _ = child.wait();
                match result {
                    Ok(summary) => {
                        let mut r = ToolResult::success(summary);
                        r.metadata.insert("backend".to_string(), "cdp".to_string());
                        Ok(r)
                    }
                    Err(e) => Ok(ToolResult::failure(
                        format!("CDP {} failed on {}: {e}", parsed.action, parsed.url),
                        Some(1),
                    )),
                }
            }
            Err(e) => {
                warn!("Chrome launch failed, falling back to HTTP: {}", e);
                fallback_http_browse(&parsed).await
            }
        }
    } else {
        // HTTP fallback for navigate / extract_text / extract_links
        fallback_http_browse(&parsed).await
    }
}

async fn fallback_http_browse(parsed: &BrowseArgs) -> Result<ToolResult, ToolkitError> {
    // reqwest::blocking cannot run inside the async runtime; offload it.
    let url = parsed.url.clone();
    let fetch = tokio::task::spawn_blocking(move || http_fetch_page(&url))
        .await
        .map_err(|e| ToolkitError::ExecutionFailed(format!("join error: {e}")))?;
    match fetch {
        Ok((body, status, headers)) => {
            let content_type = headers.get("content-type").cloned().unwrap_or_default();

            let extract_mode = parsed.extract.as_deref().unwrap_or(
                match parsed.action.as_str() {
                    "extract_text" => "text",
                    "extract_links" => "links",
                    _ => "text",
                },
            );

            let extracted = match extract_mode {
                "html" => {
                    let truncated = if body.len() > 50_000 {
                        format!("{}...", &body[..50_000])
                    } else {
                        body.clone()
                    };
                    truncated
                }
                "links" => {
                    let links = extract_links(&body);
                    links
                        .iter()
                        .map(|(href, text)| format!("- [{}]({})", text, href))
                        .collect::<Vec<_>>()
                        .join("\n")
                }
                "title" => {
                    let re = regex::Regex::new(r"(?is)<title>(.*?)</title>").unwrap();
                    re.captures(&body)
                        .and_then(|c| c.get(1))
                        .map(|m| m.as_str().trim().to_string())
                        .unwrap_or_else(|| "(no title)".to_string())
                }
                _ => {
                    // text
                    let text = extract_text(&body);
                    if text.len() > 10_000 {
                        format!("{}...", &text[..10_000])
                    } else {
                        text
                    }
                }
            };

            let mut result = ToolResult::success(format!(
                "URL: {}\nStatus: {status}\nContent-Type: {content_type}\n\n{extracted}",
                parsed.url
            ));
            result.metadata.insert("status_code".to_string(), status.to_string());
            result.metadata.insert("content_type".to_string(), content_type);
            result.metadata.insert("backend".to_string(), "http_fetch".to_string());

            Ok(result)
        }
        Err(e) => Ok(ToolResult::failure(
            format!("Failed to fetch {}: {}", parsed.url, e),
            Some(1),
        )),
    }
}

async fn execute_screenshot(args: serde_json::Value) -> Result<ToolResult, ToolkitError> {
    let url = args["url"]
        .as_str()
        .ok_or_else(|| ToolkitError::InvalidArguments("url is required".to_string()))?;
    let output_path = args["output_path"]
        .as_str()
        .unwrap_or(".ownstack/browser_screenshot.png");
    let width = args["width"].as_u64().unwrap_or(1280);
    let height = args["height"].as_u64().unwrap_or(720);

    let chrome_bin = match find_chrome_binary() {
        Some(bin) => bin,
        None => {
            return Ok(ToolResult::failure(
                "No Chrome/Chromium binary found. Install chromium or google-chrome for screenshot support.".to_string(),
                Some(1),
            ));
        }
    };

    info!("BrowserToolkit: screenshot url={url} output={output_path}");

    // Use Chrome --screenshot flag for simple page capture
    let output = Command::new(&chrome_bin)
        .args([
            "--headless",
            "--disable-gpu",
            "--no-sandbox",
            "--disable-dev-shm-usage",
            &format!("--screenshot={output_path}"),
            &format!("--window-size={width},{height}"),
            url,
        ])
        .output()
        .map_err(|e| ToolkitError::ExecutionFailed(format!("Chrome screenshot: {e}")))?;

    if output.status.success() && std::path::Path::new(output_path).exists() {
        let mut result = ToolResult::success(format!(
            "Screenshot saved to {output_path} ({}x{})",
            width, height
        ));
        result.metadata.insert("path".to_string(), output_path.to_string());
        result.metadata.insert("width".to_string(), width.to_string());
        result.metadata.insert("height".to_string(), height.to_string());
        Ok(result)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Ok(ToolResult::failure(
            format!("Screenshot failed: {}", stderr),
            Some(output.status.code().unwrap_or(1)),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_js_escape() {
        assert_eq!(js_escape("a'b"), "a\\'b");
        assert_eq!(js_escape("c\\d"), "c\\\\d");
        assert_eq!(js_escape("e\nf"), "e\\nf");
        assert_eq!(js_escape("input#name"), "input#name");
    }

    #[test]
    fn test_find_chrome_binary() {
        // Just verify it doesn't crash; may or may not find Chrome in CI
        let result = find_chrome_binary();
        println!("Chrome binary found: {:?}", result);
    }

    #[test]
    fn test_extract_text() {
        let html = r#"<html><head><title>Test</title></head>
        <body><h1>Hello</h1><p>World</p><script>alert(1)</script></body></html>"#;
        let text = extract_text(html);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
        assert!(!text.contains("alert"));
    }

    #[test]
    fn test_extract_links() {
        let html = r#"<a href="https://example.com">Example</a> and <a href="/page">Page</a>"#;
        let links = extract_links(html);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0, "https://example.com");
        assert_eq!(links[0].1, "Example");
    }

    #[tokio::test]
    async fn test_browse_url_invalid_url() {
        let toolkit = BrowserToolkit;
        let result = toolkit
            .execute("browse_url", json!({"url": "not-a-url"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_browse_url_http_fallback() {
        // Network-dependent: this exercises the spawn_blocking HTTP path.
        // It must NOT panic regardless of connectivity; a network error is
        // returned as Ok(failure) or Err, both acceptable offline.
        let toolkit = BrowserToolkit;
        let result = toolkit
            .execute(
                "browse_url",
                json!({"url": "https://httpbin.org/html", "extract": "text"}),
            )
            .await;
        match result {
            Ok(r) => println!("browse_url result: success={}", r.success),
            Err(e) => println!("browse_url offline (acceptable): {e}"),
        }
    }
}
