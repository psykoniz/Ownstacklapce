//! Artifact Manager — Extracts structured artifacts from LLM responses.
//!
//! The LLM can produce `<artifact type="PLAN">...</artifact>` tags in its text.
//! This module extracts and persists them as files in `.ownstack/artifacts/`.
//!
//! This is critical for models that don't reliably use `write_file` tool calls
//! (e.g. Deepseek V3) but naturally produce structured text with XML tags.

use regex::Regex;
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// A single extracted artifact.
#[derive(Debug, Clone)]
pub struct Artifact {
    pub artifact_type: String,
    pub name: String,
    pub content: String,
}

/// Manages extraction and persistence of agent artifacts.
pub struct ArtifactManager {
    workspace: PathBuf,
}

impl ArtifactManager {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Extract artifacts from `<artifact type="..." [name="..."]>content</artifact>` tags.
    pub fn extract_artifacts(text: &str) -> Vec<Artifact> {
        if text.is_empty() {
            return Vec::new();
        }

        let re = match Regex::new(
            r#"(?si)<artifact\s+[^>]*type="([^"]+)"(?:[^>]*name="([^"]+)")?[^>]*>(.*?)</artifact>"#,
        ) {
            Ok(regex) => regex,
            Err(err) => {
                warn!(
                    "ArtifactManager: failed to compile extraction regex: {}",
                    err
                );
                return Vec::new();
            }
        };

        re.captures_iter(text)
            .map(|cap| {
                let artifact_type =
                    cap.get(1).map_or("", |m| m.as_str()).to_string();
                let name = cap.get(2).map_or_else(
                    || artifact_type.clone(),
                    |m| m.as_str().to_string(),
                );
                let content =
                    cap.get(3).map_or("", |m| m.as_str()).trim().to_string();

                Artifact {
                    artifact_type,
                    name,
                    content,
                }
            })
            .collect()
    }

    /// Determine the filename for an artifact based on its type.
    fn artifact_filename(artifact: &Artifact) -> String {
        let type_upper = artifact.artifact_type.to_uppercase();

        match type_upper.as_str() {
            "PLAN" => "plan.md".to_string(),
            "TODO" => "todo.md".to_string(),
            "PROOF" => "proof.md".to_string(),
            "SCRATCHPAD" => "scratchpad.md".to_string(),
            _ => {
                // Sanitize name: only alphanumeric and underscores
                let safe_name: String = artifact
                    .name
                    .to_lowercase()
                    .replace(' ', "_")
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_')
                    .collect();

                let safe_name = if safe_name.is_empty() {
                    "unnamed".to_string()
                } else {
                    safe_name
                };

                if type_upper == safe_name.to_uppercase() {
                    format!("{}.md", type_upper.to_lowercase())
                } else {
                    format!("{}_{}.md", type_upper.to_lowercase(), safe_name)
                }
            }
        }
    }

    /// Save extracted artifacts to `.ownstack/artifacts/` with atomic writes.
    pub fn save_artifacts(&self, artifacts: &[Artifact]) -> Vec<String> {
        if artifacts.is_empty() {
            return Vec::new();
        }

        let artifacts_dir = self.workspace.join(".ownstack").join("artifacts");

        if let Err(e) = std::fs::create_dir_all(&artifacts_dir) {
            warn!("ArtifactManager: failed to create artifacts dir: {}", e);
            return Vec::new();
        }

        let mut saved = Vec::new();

        for artifact in artifacts {
            let filename = Self::artifact_filename(artifact);
            let target_path = artifacts_dir.join(&filename);

            match atomic_write(&target_path, &artifact.content) {
                Ok(()) => {
                    info!("ArtifactManager: saved {:?}", target_path);
                    saved.push(filename);
                }
                Err(e) => {
                    warn!("ArtifactManager: failed to save {}: {}", filename, e);
                }
            }
        }

        saved
    }

    /// Extract artifacts from text and save them. Returns the list of saved filenames.
    pub fn process_response(&self, text: &str) -> Vec<String> {
        let artifacts = Self::extract_artifacts(text);
        if !artifacts.is_empty() {
            debug!(
                "ArtifactManager: found {} artifact(s) in response",
                artifacts.len()
            );
        }
        self.save_artifacts(&artifacts)
    }
}

/// Atomic write: write to temp file then rename.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let temp_path = path.with_extension("tmp");
    std::fs::write(&temp_path, content)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_extract_single_artifact() {
        let text = r#"Here is my plan:
<artifact type="PLAN">
## Step 1
Do the thing
</artifact>
Done."#;

        let artifacts = ArtifactManager::extract_artifacts(text);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_type, "PLAN");
        assert!(artifacts[0].content.contains("Step 1"));
    }

    #[test]
    fn test_extract_with_name() {
        let text =
            r#"<artifact type="docs" name="api_reference">API docs here</artifact>"#;

        let artifacts = ArtifactManager::extract_artifacts(text);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].artifact_type, "docs");
        assert_eq!(artifacts[0].name, "api_reference");
        assert_eq!(artifacts[0].content, "API docs here");
    }

    #[test]
    fn test_extract_multiple_artifacts() {
        let text = r#"
<artifact type="PLAN">Plan content</artifact>
Some text between
<artifact type="TODO">Todo content</artifact>
"#;

        let artifacts = ArtifactManager::extract_artifacts(text);
        assert_eq!(artifacts.len(), 2);
        assert_eq!(artifacts[0].artifact_type, "PLAN");
        assert_eq!(artifacts[1].artifact_type, "TODO");
    }

    #[test]
    fn test_extract_no_artifacts() {
        let artifacts =
            ArtifactManager::extract_artifacts("Just normal text without tags");
        assert!(artifacts.is_empty());

        let artifacts_empty = ArtifactManager::extract_artifacts("");
        assert!(artifacts_empty.is_empty());
    }

    #[test]
    fn test_filename_mapping() {
        let plan = Artifact {
            artifact_type: "PLAN".into(),
            name: "PLAN".into(),
            content: String::new(),
        };
        assert_eq!(ArtifactManager::artifact_filename(&plan), "plan.md");

        let custom = Artifact {
            artifact_type: "analysis".into(),
            name: "perf report".into(),
            content: String::new(),
        };
        assert_eq!(
            ArtifactManager::artifact_filename(&custom),
            "analysis_perf_report.md"
        );
    }

    #[test]
    fn test_filename_sanitization() {
        let evil = Artifact {
            artifact_type: "docs".into(),
            name: "../../../etc/passwd".into(),
            content: String::new(),
        };
        let filename = ArtifactManager::artifact_filename(&evil);
        assert!(!filename.contains(".."));
        assert!(!filename.contains('/'));
    }

    #[test]
    fn test_save_artifacts() {
        let dir = tempdir().unwrap();
        let manager = ArtifactManager::new(dir.path().to_path_buf());

        let artifacts = vec![Artifact {
            artifact_type: "PLAN".into(),
            name: "PLAN".into(),
            content: "My plan content".into(),
        }];

        let saved = manager.save_artifacts(&artifacts);
        assert_eq!(saved, vec!["plan.md"]);

        let content = std::fs::read_to_string(
            dir.path()
                .join(".ownstack")
                .join("artifacts")
                .join("plan.md"),
        )
        .unwrap();
        assert_eq!(content, "My plan content");
    }

    #[test]
    fn test_process_response_end_to_end() {
        let dir = tempdir().unwrap();
        let manager = ArtifactManager::new(dir.path().to_path_buf());

        let text = r#"Here's my analysis:
<artifact type="PLAN">
# Implementation Plan
1. Step one
2. Step two
</artifact>
And also:
<artifact type="PROOF">All tests pass.</artifact>
"#;

        let saved = manager.process_response(text);
        assert_eq!(saved.len(), 2);
        assert!(saved.contains(&"plan.md".to_string()));
        assert!(saved.contains(&"proof.md".to_string()));
    }
}
