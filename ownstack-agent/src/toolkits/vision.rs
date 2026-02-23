//! Vision Toolkit
//!
//! Tools for capturing UI state and analyzing images.

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde::Deserialize;
use std::path::PathBuf;

use ownstack_engine::{
    vision, AuditEntry, AuditLogger, PathValidator, PolicyDecision,
};
use std::time::Instant;
use tracing::warn;

pub struct VisionToolkit {
    workspace: PathBuf,
    session_id: String,
    path_validator: PathValidator,
    audit_logger: AuditLogger,
}

impl VisionToolkit {
    pub fn new(workspace: PathBuf, session_id: String) -> Self {
        let audit_logger = AuditLogger::new(workspace.clone());
        let path_validator = PathValidator::new(workspace.clone());
        Self {
            workspace,
            session_id,
            path_validator,
            audit_logger,
        }
    }

    fn audit(
        &self,
        action: &str,
        command: &str,
        policy_decision: PolicyDecision,
        tool_name: &str,
        success: bool,
        duration_ms: u64,
        paths_accessed: Vec<String>,
    ) {
        let entry = AuditEntry {
            timestamp: String::new(),
            session_id: self.session_id.clone(),
            action: action.to_string(),
            command: command.to_string(),
            policy_decision,
            tool_name: tool_name.to_string(),
            success,
            duration_ms,
            workspace: self.workspace.to_string_lossy().to_string(),
            paths_accessed,
        };

        if let Err(err) = self.audit_logger.log(entry) {
            warn!("audit log failed: {}", err);
        }
    }
}

#[derive(Deserialize)]
struct AnalyzeImageArgs {
    image_path: String,
    prompt: String,
}

#[derive(Deserialize)]
struct CaptureUiArgs {
    panel: Option<String>,
    title: Option<String>,
}

#[derive(Deserialize)]
struct ClickUiArgs {
    x: i32,
    y: i32,
}

#[async_trait]
impl Toolkit for VisionToolkit {
    fn name(&self) -> &str {
        "vision"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "analyze_image".to_string(),
                description: "Analyze an image file using the multi-modal agent"
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "image_path": {
                            "type": "string",
                            "description": "Path to the image file (relative to workspace)"
                        },
                        "prompt": {
                            "type": "string",
                            "description": "Specific question or analysis prompt for the image"
                        }
                    },
                    "required": ["image_path", "prompt"]
                }),
            },
            ToolDef {
                name: "capture_desktop".to_string(),
                description: "Capture the entire desktop (full screen)".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {}
                }),
            },
            ToolDef {
                name: "capture_ui".to_string(),
                description:
                    "Capture the current IDE UI state (active window or specified by title)"
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "panel": {
                            "type": "string",
                            "description": "Optional panel name (for compatibility, currently captures the window)"
                        },
                        "title": {
                            "type": "string",
                            "description": "Optional substring of the window title to target (e.g. 'Lapce')"
                        }
                    }
                }),
            },
            ToolDef {
                name: "click_ui".to_string(),
                description: "Perform a native mouse click at specified relative coordinates"
                    .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "x": {
                            "type": "integer",
                            "description": "X coordinate relative to window"
                        },
                        "y": {
                            "type": "integer",
                            "description": "Y coordinate relative to window"
                        }
                    },
                    "required": ["x", "y"]
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
            "analyze_image" => {
                let start = Instant::now();
                let parsed: AnalyzeImageArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;

                let image_path = std::path::Path::new(&parsed.image_path);

                // Step 2: Path validation (GEMINI.md 6.3)
                let validated_path =
                    self.path_validator.validate(image_path).map_err(|e| {
                        self.audit(
                            "read_image",
                            &parsed.image_path,
                            PolicyDecision::Blocked,
                            "vision.analyze_image",
                            false,
                            start.elapsed().as_millis() as u64,
                            vec![parsed.image_path.clone()],
                        );
                        ToolkitError::SecurityViolation(e.to_string())
                    })?;

                let data = std::fs::read(&validated_path).map_err(|e| {
                    self.audit(
                        "read_image",
                        &parsed.image_path,
                        PolicyDecision::Auto,
                        "vision.analyze_image",
                        false,
                        start.elapsed().as_millis() as u64,
                        vec![validated_path.to_string_lossy().to_string()],
                    );
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to read image: {}",
                        e
                    ))
                })?;

                let b64 = base64_simd::STANDARD.encode_to_string(&data);
                let media_type =
                    match validated_path.extension().and_then(|s| s.to_str()) {
                        Some("png") => "image/png",
                        Some("jpg") | Some("jpeg") => "image/jpeg",
                        _ => "image/png",
                    };

                let mut result = ToolResult::success(format!(
                    "Image loaded: {}. Prompt: {}",
                    parsed.image_path, parsed.prompt
                ));
                result.metadata.insert("image_data".to_string(), b64);
                result
                    .metadata
                    .insert("media_type".to_string(), media_type.to_string());

                self.audit(
                    "read_image",
                    &parsed.image_path,
                    PolicyDecision::Auto,
                    "vision.analyze_image",
                    true,
                    start.elapsed().as_millis() as u64,
                    vec![validated_path.to_string_lossy().to_string()],
                );

                Ok(result)
            }
            "capture_ui" => {
                let start = Instant::now();
                let parsed: CaptureUiArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let capture_target = if let Some(title) = parsed.title.as_ref() {
                    format!("title:{title}")
                } else if let Some(panel) = parsed.panel.as_ref() {
                    format!("panel:{panel}")
                } else {
                    "active_window".to_string()
                };

                let rel_path = std::path::Path::new(".ownstack/ui_screenshot.png");
                let validated_path =
                    self.path_validator.validate(rel_path).map_err(|e| {
                        self.audit(
                            "capture_ui",
                            &capture_target,
                            PolicyDecision::Blocked,
                            "vision.capture_ui",
                            false,
                            start.elapsed().as_millis() as u64,
                            vec![rel_path.to_string_lossy().to_string()],
                        );
                        ToolkitError::SecurityViolation(e.to_string())
                    })?;

                if let Some(parent) = validated_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        self.audit(
                            "capture_ui",
                            &capture_target,
                            PolicyDecision::Auto,
                            "vision.capture_ui",
                            false,
                            start.elapsed().as_millis() as u64,
                            vec![validated_path.to_string_lossy().to_string()],
                        );
                        ToolkitError::ExecutionFailed(format!(
                            "Failed to create screenshot directory: {}",
                            e
                        ))
                    })?;
                }

                let capture_res = if let Some(title) = parsed.title {
                    vision::capture_window_by_title(&title, &validated_path)
                } else {
                    vision::capture_active_window(&validated_path)
                };

                capture_res.map_err(|e| {
                    self.audit(
                        "capture_ui",
                        &capture_target,
                        PolicyDecision::Auto,
                        "vision.capture_ui",
                        false,
                        start.elapsed().as_millis() as u64,
                        vec![validated_path.to_string_lossy().to_string()],
                    );
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to capture UI screenshot: {}",
                        e
                    ))
                })?;

                self.audit(
                    "capture_ui",
                    &capture_target,
                    PolicyDecision::Auto,
                    "vision.capture_ui",
                    true,
                    start.elapsed().as_millis() as u64,
                    vec![validated_path.to_string_lossy().to_string()],
                );

                let mut result = ToolResult::success(format!(
                    "UI screenshot captured to {}",
                    validated_path.display()
                ));
                result.metadata.insert(
                    "image_path".to_string(),
                    validated_path.to_string_lossy().to_string(),
                );
                Ok(result)
            }
            "capture_desktop" => {
                let start = Instant::now();
                let rel_path =
                    std::path::Path::new(".ownstack/desktop_screenshot.png");
                let validated_path = self
                    .path_validator
                    .validate(rel_path)
                    .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;

                if let Some(parent) = validated_path.parent() {
                    let _ = std::fs::create_dir_all(parent);
                }

                vision::capture_screen(&validated_path).map_err(|e| {
                    self.audit(
                        "capture_desktop",
                        "full_screen",
                        PolicyDecision::Auto,
                        "vision.capture_desktop",
                        false,
                        start.elapsed().as_millis() as u64,
                        vec![validated_path.to_string_lossy().to_string()],
                    );
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to capture desktop: {}",
                        e
                    ))
                })?;

                self.audit(
                    "capture_desktop",
                    "full_screen",
                    PolicyDecision::Auto,
                    "vision.capture_desktop",
                    true,
                    start.elapsed().as_millis() as u64,
                    vec![validated_path.to_string_lossy().to_string()],
                );

                let mut result = ToolResult::success(format!(
                    "Desktop screenshot captured to {}",
                    validated_path.display()
                ));
                result.metadata.insert(
                    "image_path".to_string(),
                    validated_path.to_string_lossy().to_string(),
                );
                Ok(result)
            }
            "click_ui" => {
                let start = Instant::now();
                let parsed: ClickUiArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;

                vision::click_active_window(parsed.x, parsed.y).map_err(|e| {
                    self.audit(
                        "click_ui",
                        &format!("click {},{}", parsed.x, parsed.y),
                        PolicyDecision::Auto,
                        "vision.click_ui",
                        false,
                        start.elapsed().as_millis() as u64,
                        vec![],
                    );
                    ToolkitError::ExecutionFailed(format!(
                        "Failed to perform native click: {}",
                        e
                    ))
                })?;

                self.audit(
                    "click_ui",
                    &format!("click {},{}", parsed.x, parsed.y),
                    PolicyDecision::Auto,
                    "vision.click_ui",
                    true,
                    start.elapsed().as_millis() as u64,
                    vec![],
                );

                Ok(ToolResult::success(format!(
                    "Performed native click at ({}, {})",
                    parsed.x, parsed.y
                )))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}
