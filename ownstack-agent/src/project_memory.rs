//! Project Memory — Reads .ownstack/rules.md
//!
//! Provides context from the project's custom rules file
//! that the agent should follow when working on the project.

use std::path::PathBuf;
use tracing::{debug, info};

/// Project Memory reader
pub struct ProjectMemory {
    workspace: PathBuf,
}

impl ProjectMemory {
    pub fn new(workspace: PathBuf) -> Self {
        Self { workspace }
    }

    /// Load .ownstack/rules.md from the workspace
    pub fn load_rules(&self) -> Option<String> {
        let rules_path = self.workspace.join(".ownstack").join("rules.md");

        if rules_path.exists() {
            match std::fs::read_to_string(&rules_path) {
                Ok(content) => {
                    info!("ProjectMemory: loaded rules from {:?} ({} bytes)",
                          rules_path, content.len());
                    Some(content)
                }
                Err(e) => {
                    debug!("ProjectMemory: failed to read rules: {}", e);
                    None
                }
            }
        } else {
            debug!("ProjectMemory: no rules.md found at {:?}", rules_path);
            None
        }
    }

    /// Load all project memory files from .ownstack/
    pub fn load_all(&self) -> ProjectContext {
        let ownstack_dir = self.workspace.join(".ownstack");
        let mut context = ProjectContext::default();

        // Rules
        context.rules = self.load_rules();

        // Stack/tech info
        let stack_path = ownstack_dir.join("stack.md");
        if stack_path.exists() {
            context.stack = std::fs::read_to_string(&stack_path).ok();
        }

        // Conventions
        let conventions_path = ownstack_dir.join("conventions.md");
        if conventions_path.exists() {
            context.conventions = std::fs::read_to_string(&conventions_path).ok();
        }

        // Custom prompts
        let prompts_path = ownstack_dir.join("prompts.md");
        if prompts_path.exists() {
            context.custom_prompts = std::fs::read_to_string(&prompts_path).ok();
        }

        context
    }

    /// Generate a system prompt augmentation from project memory
    pub fn to_system_prompt(&self) -> String {
        let ctx = self.load_all();
        let mut parts = Vec::new();

        if let Some(rules) = &ctx.rules {
            parts.push(format!("## Project Rules\n\n{}", rules));
        }
        if let Some(stack) = &ctx.stack {
            parts.push(format!("## Tech Stack\n\n{}", stack));
        }
        if let Some(conventions) = &ctx.conventions {
            parts.push(format!("## Code Conventions\n\n{}", conventions));
        }
        if let Some(prompts) = &ctx.custom_prompts {
            parts.push(format!("## Custom Instructions\n\n{}", prompts));
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("# Project Memory\n\n{}", parts.join("\n\n---\n\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;
    use std::fs;

    #[test]
    fn test_project_memory_load_rules() {
        let dir = tempdir().unwrap();
        let ownstack_dir = dir.path().join(".ownstack");
        fs::create_dir(&ownstack_dir).unwrap();
        fs::write(ownstack_dir.join("rules.md"), "# Rule 1").unwrap();

        let memory = ProjectMemory::new(dir.path().to_path_buf());
        let rules = memory.load_rules();
        assert_eq!(rules, Some("# Rule 1".to_string()));
    }

    #[test]
    fn test_project_memory_load_all() {
        let dir = tempdir().unwrap();
        let ownstack_dir = dir.path().join(".ownstack");
        fs::create_dir(&ownstack_dir).unwrap();
        fs::write(ownstack_dir.join("rules.md"), "rules").unwrap();
        fs::write(ownstack_dir.join("stack.md"), "stack").unwrap();
        fs::write(ownstack_dir.join("conventions.md"), "conventions").unwrap();
        fs::write(ownstack_dir.join("prompts.md"), "prompts").unwrap();

        let memory = ProjectMemory::new(dir.path().to_path_buf());
        let ctx = memory.load_all();
        
        assert_eq!(ctx.rules, Some("rules".to_string()));
        assert_eq!(ctx.stack, Some("stack".to_string()));
        assert_eq!(ctx.conventions, Some("conventions".to_string()));
        assert_eq!(ctx.custom_prompts, Some("prompts".to_string()));
    }

    #[test]
    fn test_to_system_prompt() {
        let dir = tempdir().unwrap();
        let ownstack_dir = dir.path().join(".ownstack");
        fs::create_dir(&ownstack_dir).unwrap();
        fs::write(ownstack_dir.join("rules.md"), "Project rules here").unwrap();

        let memory = ProjectMemory::new(dir.path().to_path_buf());
        let prompt = memory.to_system_prompt();
        
        assert!(prompt.contains("# Project Memory"));
        assert!(prompt.contains("## Project Rules"));
        assert!(prompt.contains("Project rules here"));
    }

    #[test]
    fn test_project_memory_empty() {
        let dir = tempdir().unwrap();
        let memory = ProjectMemory::new(dir.path().to_path_buf());
        assert_eq!(memory.load_rules(), None);
        assert_eq!(memory.to_system_prompt(), "");
    }
}

/// Aggregated project context
#[derive(Debug, Default)]
pub struct ProjectContext {
    pub rules: Option<String>,
    pub stack: Option<String>,
    pub conventions: Option<String>,
    pub custom_prompts: Option<String>,
}
