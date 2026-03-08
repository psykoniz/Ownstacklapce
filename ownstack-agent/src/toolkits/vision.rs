//! Vision Toolkit
//!
//! Tools for capturing UI state and analyzing images.
//! Routes images to the multi-modal LLM for real analysis when a provider is configured.

use async_trait::async_trait;
use sha2::{Digest, Sha256};
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::warn;

use ownstack_engine::{AuditEntry, AuditLogger, PathValidator, PolicyDecision};

use crate::provider::{
    ContentPart, ImageSource, LlmMessage, LlmProvider, MessageContent, ProviderOptions,
};

use super::{ToolDef, ToolResult, Toolkit, ToolkitError};

const MAX_INLINE_IMAGE_BYTES: usize = 2 * 1024 * 1024;

pub struct VisionToolkit {
    workspace: PathBuf,
    session_id: String,
    path_validator: PathValidator,
    audit_logger: AuditLogger,
    provider: Option<Arc<dyn LlmProvider + Send + Sync>>,
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
            provider: None,
        }
    }

    pub fn with_provider(
        mut self,
        provider: Arc<dyn LlmProvider + Send + Sync>,
    ) -> Self {
        self.provider = Some(provider);
        self
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
                description: "Analyze an image file using the multi-modal LLM agent"
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
                name: "capture_ui".to_string(),
                description:
                    "Capture the current IDE UI state (panel or full window)"
                        .to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "panel": {
                            "type": "string",
                            "description": "Optional panel name (e.g., 'terminal', 'editor', 'chat')"
                        }
                    }
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
                let image_sha256 = format!("{:x}", Sha256::digest(&data));
                let media_type =
                    match validated_path.extension().and_then(|s| s.to_str()) {
                        Some("png") => "image/png",
                        Some("jpg") | Some("jpeg") => "image/jpeg",
                        Some("gif") => "image/gif",
                        Some("webp") => "image/webp",
                        _ => "image/png",
                    };

                // Route to multimodal LLM if provider is available
                let analysis_text = if let Some(provider) = &self.provider {
                    let messages = vec![LlmMessage {
                        role: crate::provider::Role::User,
                        content: MessageContent::Parts(vec![
                            ContentPart::Image {
                                source: ImageSource {
                                    type_: "base64".to_string(),
                                    media_type: media_type.to_string(),
                                    data: b64.clone(),
                                },
                            },
                            ContentPart::Text {
                                text: parsed.prompt.clone(),
                            },
                        ]),
                        tool_call_id: None,
                        tool_calls: None,
                    }];

                    match provider
                        .complete(messages, None, ProviderOptions::default())
                        .await
                    {
                        Ok(response) => response.content.unwrap_or_else(|| {
                            format!(
                                "Image loaded: {}. No analysis returned.",
                                parsed.image_path
                            )
                        }),
                        Err(err) => {
                            warn!("Vision LLM analysis failed: {err}");
                            format!(
                                "Image loaded: {}. LLM analysis failed: {err}. Prompt: {}",
                                parsed.image_path, parsed.prompt
                            )
                        }
                    }
                } else {
                    format!(
                        "Image loaded: {}. No LLM provider configured for multimodal analysis. Prompt: {}",
                        parsed.image_path, parsed.prompt
                    )
                };

                let mut result = ToolResult::success(analysis_text);
                if data.len() <= MAX_INLINE_IMAGE_BYTES {
                    result.metadata.insert("image_data".to_string(), b64);
                } else {
                    result.metadata.insert(
                        "image_data_omitted".to_string(),
                        "true".to_string(),
                    );
                }
                result
                    .metadata
                    .insert("image_bytes".to_string(), data.len().to_string());
                result
                    .metadata
                    .insert("image_sha256".to_string(), image_sha256);
                result
                    .metadata
                    .insert("media_type".to_string(), media_type.to_string());
                result.metadata.insert(
                    "llm_analyzed".to_string(),
                    if self.provider.is_some() { "true" } else { "false" }.to_string(),
                );

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

                let ownstack_dir = self.workspace.join(".ownstack");
                let snapshot_rel =
                    std::path::Path::new(".ownstack/ui_snapshot.json");
                let screenshot_rel =
                    std::path::Path::new(".ownstack/ui_screenshot.png");

                let snapshot_path = self
                    .path_validator
                    .validate(snapshot_rel)
                    .map_err(|e| ToolkitError::SecurityViolation(e.to_string()))?;
                let screenshot_path =
                    self.path_validator.validate(screenshot_rel).map_err(|e| {
                        ToolkitError::SecurityViolation(e.to_string())
                    })?;

                if let Err(err) = std::fs::create_dir_all(&ownstack_dir) {
                    return Ok(ToolResult::failure(
                        format!("Failed to create .ownstack directory: {}", err),
                        None,
                    ));
                }

                let snapshot_json = match std::fs::read_to_string(&snapshot_path) {
                    Ok(content) => content,
                    Err(err) => {
                        self.audit(
                            "capture_ui",
                            "capture_ui",
                            PolicyDecision::Auto,
                            "vision.capture_ui",
                            false,
                            start.elapsed().as_millis() as u64,
                            vec![snapshot_path.to_string_lossy().to_string()],
                        );
                        return Ok(ToolResult::failure(
                            format!(
                                "UI snapshot metadata missing: {}. Trigger UiSnapshotRequest first.",
                                err
                            ),
                            None,
                        ));
                    }
                };

                let screenshot_result =
                    ownstack_engine::vision::capture_active_window(&screenshot_path);

                let panel_name = parsed.panel.unwrap_or_else(|| "all".to_string());
                let mut result = if screenshot_result.is_ok() {
                    ToolResult::success(format!(
                        "UI capture ready (panel: {}).",
                        panel_name
                    ))
                } else {
                    ToolResult::success(format!(
                        "UI snapshot metadata captured (panel: {}). Screenshot unavailable on this platform.",
                        panel_name
                    ))
                };

                result.metadata.insert(
                    "snapshot_path".to_string(),
                    snapshot_path.to_string_lossy().to_string(),
                );
                result
                    .metadata
                    .insert("snapshot_json".to_string(), snapshot_json);
                result.metadata.insert(
                    "screenshot_path".to_string(),
                    screenshot_path.to_string_lossy().to_string(),
                );
                if let Err(err) = screenshot_result {
                    result.metadata.insert("screenshot_error".to_string(), err);
                }

                self.audit(
                    "capture_ui",
                    "capture_ui",
                    PolicyDecision::Auto,
                    "vision.capture_ui",
                    true,
                    start.elapsed().as_millis() as u64,
                    vec![
                        snapshot_path.to_string_lossy().to_string(),
                        screenshot_path.to_string_lossy().to_string(),
                    ],
                );

                Ok(result)
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::VisionToolkit;
    use crate::toolkits::Toolkit;

    #[tokio::test]
    async fn capture_ui_reports_missing_snapshot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let toolkit =
            VisionToolkit::new(dir.path().to_path_buf(), "sess-test".to_string());

        let result = toolkit
            .execute("capture_ui", serde_json::json!({}))
            .await
            .expect("capture_ui call should not fail at protocol layer");

        assert!(!result.success);
        assert!(result.stderr.contains("UI snapshot metadata missing"));
    }
}
