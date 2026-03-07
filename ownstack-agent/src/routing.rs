use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{debug, error};

/// Configuration for model routing
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelRoutingConfig {
    /// Default model to use if no specific rule matches
    pub default: Option<String>,
    /// Model mapping based on agent role (planner, worker, critic)
    pub roles: HashMap<String, String>,
    /// Model mapping based on task type (refactoring, docs, etc.)
    pub tasks: HashMap<String, String>,
    /// OpenRouter provider routing preferences (order, sort, allow_fallbacks, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub openrouter_provider: Option<serde_json::Value>,
}

pub struct ModelRouter {
    config: ModelRoutingConfig,
    config_path: PathBuf,
}

impl ModelRouter {
    pub fn new(workspace: &Path) -> Self {
        let config_path = workspace.join(".ownstack").join("routing.json");
        let mut router = Self {
            config: ModelRoutingConfig::default(),
            config_path,
        };
        router.reload();
        router
    }

    /// Load or reload the routing configuration from disk
    pub fn reload(&mut self) {
        if !self.config_path.exists() {
            debug!(
                "ModelRouter: no config found at {:?}, using defaults",
                self.config_path
            );
            self.config = ModelRoutingConfig::default();
            return;
        }

        let content = match std::fs::read_to_string(&self.config_path) {
            Ok(content) => content,
            Err(e) => {
                error!(
                    "ModelRouter: failed to read config at {:?}: {}",
                    self.config_path, e
                );
                return;
            }
        };

        // Accept UTF-8 BOM produced by some Windows editors/tools.
        let content = content.trim_start_matches('\u{feff}');

        if content.trim().is_empty() {
            debug!(
                "ModelRouter: config at {:?} is empty, using defaults",
                self.config_path
            );
            self.config = ModelRoutingConfig::default();
            return;
        }

        match serde_json::from_str::<ModelRoutingConfig>(&content) {
            Ok(config) => {
                debug!("ModelRouter: loaded config from {:?}", self.config_path);
                self.config = config;
            }
            Err(e) => {
                error!(
                    "ModelRouter: failed to parse config at {:?}: {}",
                    self.config_path, e
                );
            }
        }
    }

    /// Resolve the best model for a given role and task
    pub fn route(&self, role: &str, task_type: Option<&str>) -> Option<String> {
        // 1. Check specific task type override first
        if let Some(task) = task_type {
            if let Some(model) = self.config.tasks.get(task) {
                debug!("ModelRouter: routed task '{}' to model '{}'", task, model);
                return Some(model.clone());
            }
        }

        // 2. Check role override
        if let Some(model) = self.config.roles.get(role) {
            debug!("ModelRouter: routed role '{}' to model '{}'", role, model);
            return Some(model.clone());
        }

        // 3. Fallback to default
        if let Some(ref default) = self.config.default {
            debug!("ModelRouter: using default model '{}'", default);
            return Some(default.clone());
        }

        None
    }

    /// Get OpenRouter specific provider routing preferences if any
    pub fn openrouter_provider_prefs(&self) -> Option<serde_json::Value> {
        self.config.openrouter_provider.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_priority_is_task_then_role_then_default() {
        let mut config = ModelRoutingConfig::default();
        config.default = Some("default-model".to_string());
        config
            .roles
            .insert("worker".to_string(), "role-model".to_string());
        config
            .tasks
            .insert("refactoring".to_string(), "task-model".to_string());

        let router = ModelRouter {
            config,
            config_path: PathBuf::new(),
        };

        assert_eq!(
            router.route("worker", Some("refactoring")),
            Some("task-model".to_string())
        );
        assert_eq!(
            router.route("worker", Some("unknown-task")),
            Some("role-model".to_string())
        );
        assert_eq!(
            router.route("unknown-role", Some("unknown-task")),
            Some("default-model".to_string())
        );
    }

    #[test]
    fn reload_reads_workspace_config() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ownstack_dir = temp.path().join(".ownstack");
        std::fs::create_dir_all(&ownstack_dir).expect("create .ownstack");
        std::fs::write(
            ownstack_dir.join("routing.json"),
            r#"{
                "default": "gpt-4o",
                "roles": {"planner": "gpt-4o-mini"},
                "tasks": {"documentation": "gpt-4o-mini"}
            }"#,
        )
        .expect("write routing config");

        let router = ModelRouter::new(temp.path());

        assert_eq!(
            router.route("planner", Some("planning")),
            Some("gpt-4o-mini".to_string())
        );
        assert_eq!(
            router.route("worker", Some("documentation")),
            Some("gpt-4o-mini".to_string())
        );
        assert_eq!(
            router.route("worker", Some("unknown")),
            Some("gpt-4o".to_string())
        );
    }

    #[test]
    fn reload_empty_file_falls_back_to_defaults() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ownstack_dir = temp.path().join(".ownstack");
        std::fs::create_dir_all(&ownstack_dir).expect("create .ownstack");
        std::fs::write(ownstack_dir.join("routing.json"), "")
            .expect("write empty config");

        let router = ModelRouter::new(temp.path());
        assert_eq!(router.route("worker", Some("documentation")), None);
    }

    #[test]
    fn reload_supports_utf8_bom() {
        let temp = tempfile::tempdir().expect("tempdir");
        let ownstack_dir = temp.path().join(".ownstack");
        std::fs::create_dir_all(&ownstack_dir).expect("create .ownstack");

        let with_bom = format!(
            "{}{}",
            '\u{feff}',
            r#"{
                "default": "model-with-bom",
                "roles": {"worker": "worker-with-bom"},
                "tasks": {}
            }"#
        );
        std::fs::write(ownstack_dir.join("routing.json"), with_bom)
            .expect("write bom config");

        let router = ModelRouter::new(temp.path());
        assert_eq!(
            router.route("worker", Some("unknown")),
            Some("worker-with-bom".to_string())
        );
    }
}
