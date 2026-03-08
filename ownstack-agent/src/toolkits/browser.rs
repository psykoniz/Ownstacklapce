//! Browser toolkit for web content fetch and screenshots.
//!
//! Notes:
//! - `browse_url` supports navigate/extract modes via HTTP fetch.
//! - `click`/`type` are intentionally not implemented yet (no fake success).
//! - `browser_screenshot` validates output paths against workspace boundaries.

use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use ownstack_engine::PathValidator;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use tracing::info;

/// Locate a usable Chromium/Chrome binary on the system.
fn find_chrome_binary() -> Option<String> {
    let candidates = [
        // Linux
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
        // Windows
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
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

fn http_fetch_page(url: &str) -> Result<(String, u16, HashMap<String, String>), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent("OwnStack-Agent/0.1 (browser toolkit)")
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

    let body = resp.text().map_err(|e| format!("Read body: {e}"))?;
    let body = if body.len() > 100_000 {
        format!("{}... [truncated, {} bytes total]", &body[..100_000], body.len())
    } else {
        body
    };

    Ok((body, status, headers))
}

fn extract_text(html: &str) -> String {
    let re_script = regex::Regex::new(r"(?is)<(script|style)[^>]*>.*?</\1>").unwrap();
    let cleaned = re_script.replace_all(html, "");

    let re_tags = regex::Regex::new(r"<[^>]+>").unwrap();
    let text = re_tags.replace_all(&cleaned, " ");

    let re_ws = regex::Regex::new(r"\s+").unwrap();
    let text = re_ws.replace_all(&text, " ");
    text.trim().to_string()
}

fn extract_links(html: &str) -> Vec<(String, String)> {
    let re = regex::Regex::new(r#"<a\s[^>]*href=\"([^\"]*)\"[^>]*>(.*?)</a>"#).unwrap();
    re.captures_iter(html)
        .take(50)
        .filter_map(|cap| {
            let href = cap.get(1)?.as_str().to_string();
            let text = extract_text(cap.get(2)?.as_str());
            Some((href, text))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
struct BrowseArgs {
    url: String,
    #[serde(default = "default_action")]
    action: String,
    #[serde(default)]
    extract: Option<String>,
}

fn default_action() -> String {
    "navigate".to_string()
}

pub struct BrowserToolkit {
    workspace: PathBuf,
    path_validator: PathValidator,
}

impl BrowserToolkit {
    pub fn new(workspace: PathBuf) -> Self {
        let path_validator = PathValidator::new(workspace.clone());
        Self {
            workspace,
            path_validator,
        }
    }

    fn validate_url(url: &str) -> Result<(), ToolkitError> {
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Err(ToolkitError::InvalidArguments(
                "URL must start with http:// or https://".to_string(),
            ));
        }
        Ok(())
    }

    async fn execute_browse_url(
        &self,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        let parsed: BrowseArgs = serde_json::from_value(args)
            .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;

        Self::validate_url(&parsed.url)?;
        info!(
            "BrowserToolkit: browse_url action={} url={}",
            parsed.action, parsed.url
        );

        if matches!(parsed.action.as_str(), "click" | "type") {
            return Ok(ToolResult::failure(
                "Interactive actions 'click' and 'type' are not implemented yet. Use navigate/extract or browser_screenshot."
                    .to_string(),
                Some(2),
            ));
        }

        if parsed.action == "screenshot" {
            return Ok(ToolResult::failure(
                "Use tool 'browser_screenshot' for screenshots.".to_string(),
                Some(2),
            ));
        }

        match http_fetch_page(&parsed.url) {
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
                        if body.len() > 50_000 {
                            format!("{}...", &body[..50_000])
                        } else {
                            body.clone()
                        }
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

    async fn execute_screenshot(
        &self,
        args: serde_json::Value,
    ) -> Result<ToolResult, ToolkitError> {
        let url = args["url"].as_str().ok_or_else(|| {
            ToolkitError::InvalidArguments("url is required".to_string())
        })?;
        Self::validate_url(url)?;

        let output_path_raw = args["output_path"]
            .as_str()
            .unwrap_or(".ownstack/browser_screenshot.png");
        let width = args["width"].as_u64().unwrap_or(1280);
        let height = args["height"].as_u64().unwrap_or(720);

        let output_path = self
            .path_validator
            .validate(Path::new(output_path_raw))
            .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;

        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                ToolkitError::ExecutionFailed(format!(
                    "Failed to create screenshot directory: {e}"
                ))
            })?;
        }

        let chrome_bin = match find_chrome_binary() {
            Some(bin) => bin,
            None => {
                return Ok(ToolResult::failure(
                    "No Chrome/Chromium binary found. Install chromium or google-chrome for screenshot support."
                        .to_string(),
                    Some(1),
                ));
            }
        };

        info!(
            "BrowserToolkit: screenshot url={} output={} workspace={}",
            url,
            output_path.display(),
            self.workspace.display()
        );

        let allow_no_sandbox = std::env::var("OWNSTACK_ALLOW_CHROME_NO_SANDBOX")
            .ok()
            .map(|v| {
                let lower = v.to_ascii_lowercase();
                matches!(lower.as_str(), "1" | "true" | "yes")
            })
            .unwrap_or(false);

        let mut cmd = Command::new(&chrome_bin);
        cmd.arg("--headless")
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .arg(format!("--screenshot={}", output_path.display()))
            .arg(format!("--window-size={width},{height}"))
            .arg(url);
        if allow_no_sandbox {
            cmd.arg("--no-sandbox");
        }

        let output = cmd.output().map_err(|e| {
            ToolkitError::ExecutionFailed(format!("Chrome screenshot failed to start: {e}"))
        })?;

        if output.status.success() && output_path.exists() {
            let mut result = ToolResult::success(format!(
                "Screenshot saved to {} ({}x{})",
                output_path.display(),
                width,
                height
            ));
            result
                .metadata
                .insert("path".to_string(), output_path.to_string_lossy().to_string());
            result.metadata.insert("width".to_string(), width.to_string());
            result.metadata.insert("height".to_string(), height.to_string());
            result.metadata.insert(
                "no_sandbox".to_string(),
                if allow_no_sandbox { "true" } else { "false" }.to_string(),
            );
            Ok(result)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Ok(ToolResult::failure(
                format!("Screenshot failed: {}", stderr),
                Some(output.status.code().unwrap_or(1)),
            ))
        }
    }
}

impl Default for BrowserToolkit {
    fn default() -> Self {
        let workspace = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(workspace)
    }
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
                description: "Navigate to a URL, fetch content, and extract information via HTTP."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "url": {"type": "string", "description": "The URL to visit"},
                        "action": {
                            "type": "string",
                            "enum": ["navigate", "click", "type", "screenshot", "extract_text", "extract_links"],
                            "default": "navigate",
                            "description": "Action to perform on the page"
                        },
                        "extract": {
                            "type": "string",
                            "enum": ["text", "links", "html", "title"],
                            "description": "What to extract from the page"
                        },
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
                        "output_path": {"type": "string", "description": "Where to save the screenshot (PNG, workspace-relative preferred)"},
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
            "browse_url" => self.execute_browse_url(args).await,
            "browser_screenshot" => self.execute_screenshot(args).await,
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_chrome_binary() {
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
        let toolkit = BrowserToolkit::default();
        let result = toolkit
            .execute("browse_url", json!({"url": "not-a-url"}))
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_browse_url_click_returns_not_implemented() {
        let toolkit = BrowserToolkit::default();
        let result = toolkit
            .execute(
                "browse_url",
                json!({"url": "https://example.com", "action": "click"}),
            )
            .await
            .expect("result");
        assert!(!result.success);
        assert!(result.stderr.contains("not implemented"));
    }

    #[tokio::test]
    async fn test_browse_url_http_fallback() {
        // This test actually fetches from the network; skip in offline CI.
        let toolkit = BrowserToolkit::default();
        let result = toolkit
            .execute(
                "browse_url",
                json!({"url": "https://httpbin.org/html", "extract": "text"}),
            )
            .await;
        if let Ok(r) = result {
            println!("browse_url result: success={}", r.success);
        }
    }

    #[tokio::test]
    async fn test_screenshot_rejects_traversal_output_path() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let toolkit = BrowserToolkit::new(tmp.path().to_path_buf());

        let result = toolkit
            .execute(
                "browser_screenshot",
                json!({
                    "url": "https://example.com",
                    "output_path": "../escape.png"
                }),
            )
            .await;

        assert!(matches!(result, Err(ToolkitError::SecurityViolation(_))));
    }
}
