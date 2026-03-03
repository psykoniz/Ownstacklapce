//! Project Memory — Smart Rules Engine with Priority & Hot-Reload
//!
//! Reads `.ownstack/rules.md` or `AGENTS.md` and injects project-specific
//! rules into agent prompts with structured parsing, priority weighting,
//! and keyword relevance boost.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::SystemTime;
use tracing::{debug, info};

// ─── Structured Rules ───────────────────────────────────────────

/// Parsed project rules from `.ownstack/rules.md` or `AGENTS.md`
#[derive(Debug, Default, Clone)]
pub struct ProjectRules {
    pub forbidden: Vec<String>,
    pub coding_style: Vec<String>,
    pub testing: Vec<String>,
    pub preferences: Vec<String>,
    pub libraries: Vec<String>,
    pub knowledge: Vec<String>,
    pub custom_sections: HashMap<String, Vec<String>>,
    pub content_hash: String,
}

/// A rule with its computed priority score
#[derive(Debug, Clone)]
pub struct PrioritizedRule {
    pub text: String,
    pub score: f32,
}

/// Priority weights for each section (higher = injected first)
const W_FORBIDDEN: f32 = 1.0;
const W_CODING_STYLE: f32 = 0.8;
const W_TESTING: f32 = 0.7;
const W_PREFERENCES: f32 = 0.6;
const W_LIBRARIES: f32 = 0.5;
const W_KNOWLEDGE: f32 = 0.4;
const W_CUSTOM: f32 = 0.3;

impl ProjectRules {
    /// Get rules sorted by priority, optionally boosted by relevance to the task.
    pub fn get_prioritized_rules(&self, task_context: &str) -> Vec<PrioritizedRule> {
        let mut all: Vec<PrioritizedRule> = Vec::new();

        for r in &self.forbidden {
            all.push(PrioritizedRule {
                text: format!("❌ FORBIDDEN: {r}"),
                score: W_FORBIDDEN,
            });
        }
        for r in &self.coding_style {
            all.push(PrioritizedRule {
                text: format!("Style: {r}"),
                score: W_CODING_STYLE,
            });
        }
        for r in &self.testing {
            all.push(PrioritizedRule {
                text: format!("Test: {r}"),
                score: W_TESTING,
            });
        }
        for r in &self.preferences {
            all.push(PrioritizedRule {
                text: format!("Prefer: {r}"),
                score: W_PREFERENCES,
            });
        }
        for r in &self.libraries {
            all.push(PrioritizedRule {
                text: format!("Use: {r}"),
                score: W_LIBRARIES,
            });
        }
        for r in &self.knowledge {
            all.push(PrioritizedRule {
                text: format!("💡 {r}"),
                score: W_KNOWLEDGE,
            });
        }
        for (_section, items) in &self.custom_sections {
            for r in items {
                all.push(PrioritizedRule {
                    text: r.clone(),
                    score: W_CUSTOM,
                });
            }
        }

        // Keyword relevance boost
        if !task_context.is_empty() {
            let task_lower = task_context.to_lowercase();
            let keywords: Vec<&str> = task_lower
                .split_whitespace()
                .filter(|kw| kw.len() > 3)
                .collect();

            for rule in &mut all {
                let rule_lower = rule.text.to_lowercase();
                let boost: f32 = keywords
                    .iter()
                    .filter(|kw| rule_lower.contains(*kw))
                    .count() as f32
                    * 0.1;
                rule.score = (rule.score + boost).min(1.0);
            }
        }

        all.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all
    }

    /// Convert rules to an optimized system prompt section.
    pub fn to_system_prompt_section(
        &self,
        task_context: &str,
        max_rules: usize,
    ) -> String {
        let prioritized = self.get_prioritized_rules(task_context);

        if prioritized.is_empty() {
            return String::new();
        }

        let mut lines =
            vec!["## Project Rules (from .ownstack/rules.md)".to_string()];

        for rule in prioritized.iter().take(max_rules) {
            if rule.score >= 0.9 {
                lines.push(format!("- 🚨 {}", rule.text));
            } else {
                lines.push(format!("- {}", rule.text));
            }
        }

        if prioritized.len() > max_rules {
            lines.push(format!(
                "\n_({} lower-priority rules omitted)_",
                prioritized.len() - max_rules
            ));
        }

        lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.forbidden.is_empty()
            && self.coding_style.is_empty()
            && self.testing.is_empty()
            && self.preferences.is_empty()
            && self.libraries.is_empty()
            && self.knowledge.is_empty()
            && self.custom_sections.is_empty()
    }
}

// ─── Markdown Parser ─────────────────────────────────────────────

/// Section name mapping (case-insensitive)
fn section_key(header: &str) -> Option<&'static str> {
    let lower = header.to_lowercase();
    match lower.as_str() {
        "coding style" | "style" | "coding guidelines" => Some("coding_style"),
        "forbidden" | "never" | "don't" | "don'ts" => Some("forbidden"),
        "preferences" | "prefer" => Some("preferences"),
        "libraries" | "deps" | "dependencies" => Some("libraries"),
        "testing" | "tests" | "testing instructions" => Some("testing"),
        "knowledge" | "memory" | "discoveries" => Some("knowledge"),
        "security" | "security policy" => Some("forbidden"), // treat security as forbidden-level
        _ => None,
    }
}

/// Parse markdown content into structured rules.
pub fn parse_rules_md(content: &str) -> ProjectRules {
    let hash = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        content.hash(&mut h);
        format!("{:x}", h.finish())
    };

    let mut rules = ProjectRules {
        content_hash: hash[..8.min(hash.len())].to_string(),
        ..Default::default()
    };

    let mut current_section: Option<String> = None;
    let mut current_items: Vec<String> = Vec::new();

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Detect ## or # headers
        if let Some(header_text) = trimmed
            .strip_prefix("## ")
            .or_else(|| trimmed.strip_prefix("# "))
        {
            // Save previous section
            if let Some(ref section) = current_section {
                save_section(&mut rules, section, &current_items);
            }
            current_section = Some(header_text.trim().to_string());
            current_items.clear();
            continue;
        }

        // Detect list items (- or *)
        if let Some(item_text) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            if current_section.is_some() {
                let mut text = item_text.trim().to_string();
                // Handle [P0] priority markers
                if text.to_uppercase().contains("[P0]") {
                    text = format!(
                        "🚨 {}",
                        text.replace("[P0]", "")
                            .replace("[p0]", "")
                            .trim()
                            .to_string()
                    );
                }
                current_items.push(text);
            }
        }
    }

    // Save last section
    if let Some(ref section) = current_section {
        save_section(&mut rules, section, &current_items);
    }

    rules
}

fn save_section(rules: &mut ProjectRules, section_name: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    match section_key(section_name) {
        Some("coding_style") => rules.coding_style.extend(items.iter().cloned()),
        Some("forbidden") => rules.forbidden.extend(items.iter().cloned()),
        Some("preferences") => rules.preferences.extend(items.iter().cloned()),
        Some("libraries") => rules.libraries.extend(items.iter().cloned()),
        Some("testing") => rules.testing.extend(items.iter().cloned()),
        Some("knowledge") => rules.knowledge.extend(items.iter().cloned()),
        _ => {
            rules
                .custom_sections
                .entry(section_name.to_string())
                .or_default()
                .extend(items.iter().cloned());
        }
    }
}

// ─── Rules Loader with Hot-Reload ────────────────────────────────

/// Loads rules from `AGENTS.md` (preferred) or `.ownstack/rules.md` (fallback).
/// Caches the parsed result and auto-reloads when the file changes on disk.
pub struct RulesLoader {
    workspace: PathBuf,
    cached_rules: Option<ProjectRules>,
    cached_mtime: Option<SystemTime>,
    active_path: Option<PathBuf>,
}

impl RulesLoader {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            cached_rules: None,
            cached_mtime: None,
            active_path: None,
        }
    }

    /// Resolve which rules file to use: AGENTS.md > .ownstack/rules.md
    fn resolve_path(&self) -> Option<PathBuf> {
        let agents_md = self.workspace.join("AGENTS.md");
        if agents_md.exists() {
            return Some(agents_md);
        }
        let rules_md = self.workspace.join(".ownstack").join("rules.md");
        if rules_md.exists() {
            return Some(rules_md);
        }
        None
    }

    /// Get rules, reloading automatically if the file has changed.
    pub fn get_rules(&mut self) -> ProjectRules {
        let Some(path) = self.resolve_path() else {
            return ProjectRules::default();
        };

        let current_mtime = std::fs::metadata(&path).and_then(|m| m.modified()).ok();

        let path_changed = self.active_path.as_ref() != Some(&path);
        let mtime_changed = current_mtime != self.cached_mtime;

        if !path_changed && !mtime_changed {
            if let Some(ref cached) = self.cached_rules {
                return cached.clone();
            }
        }

        // Reload
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                info!(
                    "RulesLoader: (re)loaded rules from {:?} ({} bytes)",
                    path,
                    content.len()
                );
                let parsed = parse_rules_md(&content);
                self.cached_rules = Some(parsed.clone());
                self.cached_mtime = current_mtime;
                self.active_path = Some(path);
                parsed
            }
            Err(e) => {
                debug!("RulesLoader: failed to read {:?}: {}", path, e);
                ProjectRules::default()
            }
        }
    }

    pub fn has_rules(&self) -> bool {
        self.resolve_path().is_some()
    }
}

// ─── ProjectMemory (backward-compatible facade) ──────────────────

/// Project Memory reader — backward-compatible API.
///
/// For new code, prefer using `RulesLoader` directly for hot-reload support.
pub struct ProjectMemory {
    workspace: PathBuf,
}

/// Aggregated project context
#[derive(Debug, Default)]
pub struct ProjectContext {
    pub rules: Option<String>,
    pub stack: Option<String>,
    pub conventions: Option<String>,
    pub custom_prompts: Option<String>,
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
                    info!(
                        "ProjectMemory: loaded rules from {:?} ({} bytes)",
                        rules_path,
                        content.len()
                    );
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

        context.rules = self.load_rules();

        let stack_path = ownstack_dir.join("stack.md");
        if stack_path.exists() {
            context.stack = std::fs::read_to_string(&stack_path).ok();
        }

        let conventions_path = ownstack_dir.join("conventions.md");
        if conventions_path.exists() {
            context.conventions = std::fs::read_to_string(&conventions_path).ok();
        }

        let prompts_path = ownstack_dir.join("prompts.md");
        if prompts_path.exists() {
            context.custom_prompts = std::fs::read_to_string(&prompts_path).ok();
        }

        context
    }

    /// Generate a system prompt augmentation from project memory.
    ///
    /// This now uses the advanced priority rules engine when a rules file exists.
    pub fn to_system_prompt(&self) -> String {
        let ctx = self.load_all();
        let mut parts = Vec::new();

        // Use structured parsing for rules if available
        if let Some(ref rules_content) = ctx.rules {
            let parsed = parse_rules_md(rules_content);
            if !parsed.is_empty() {
                parts.push(parsed.to_system_prompt_section("", 30));
            } else {
                // Fallback: inject as raw text
                parts.push(format!("## Project Rules\n\n{}", rules_content));
            }
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

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    // Backward-compatible tests
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

    // New Priority Rules Engine tests
    #[test]
    fn test_parse_rules_md_sections() {
        let content = "# Forbidden\n- Never use eval()\n- No hardcoded secrets\n\n# Coding Style\n- Use snake_case\n\n# Testing\n- Always write unit tests\n";
        let rules = parse_rules_md(content);

        assert_eq!(rules.forbidden.len(), 2);
        assert_eq!(rules.coding_style.len(), 1);
        assert_eq!(rules.testing.len(), 1);
        assert!(rules.forbidden[0].contains("eval"));
        assert!(rules.coding_style[0].contains("snake_case"));
    }

    #[test]
    fn test_parse_rules_md_case_insensitive_headers() {
        let content = "## Style\n- Clean code\n\n## Never\n- Use println\n";
        let rules = parse_rules_md(content);

        assert_eq!(rules.coding_style, vec!["Clean code"]);
        assert_eq!(rules.forbidden, vec!["Use println"]);
    }

    #[test]
    fn test_parse_rules_md_custom_sections() {
        let content = "# Architecture\n- Hexagonal pattern\n- Use traits\n";
        let rules = parse_rules_md(content);

        assert!(rules.custom_sections.contains_key("Architecture"));
        assert_eq!(rules.custom_sections["Architecture"].len(), 2);
    }

    #[test]
    fn test_parse_rules_md_p0_marker() {
        let content = "# Forbidden\n- [P0] Never delete production data\n";
        let rules = parse_rules_md(content);

        assert!(rules.forbidden[0].contains("🚨"));
        assert!(rules.forbidden[0].contains("Never delete production data"));
    }

    #[test]
    fn test_priority_ordering() {
        let content = "# Knowledge\n- Use async/await\n\n# Forbidden\n- Never panic\n\n# Preferences\n- Prefer tokio\n";
        let rules = parse_rules_md(content);
        let prioritized = rules.get_prioritized_rules("");

        // Forbidden (1.0) should come before Knowledge (0.4)
        assert!(prioritized[0].score >= prioritized.last().unwrap().score);
        assert!(prioritized[0].text.contains("FORBIDDEN"));
    }

    #[test]
    fn test_keyword_relevance_boost() {
        let content = "# Coding Style\n- Use async/await for database calls\n- Keep functions short\n";
        let rules = parse_rules_md(content);

        let without_context = rules.get_prioritized_rules("");
        let with_context =
            rules.get_prioritized_rules("fix the database connection");

        // The database rule should be boosted when context mentions "database"
        let db_rule_score_before = without_context
            .iter()
            .find(|r| r.text.contains("database"))
            .map(|r| r.score)
            .unwrap_or(0.0);

        let db_rule_score_after = with_context
            .iter()
            .find(|r| r.text.contains("database"))
            .map(|r| r.score)
            .unwrap_or(0.0);

        assert!(db_rule_score_after >= db_rule_score_before);
    }

    #[test]
    fn test_system_prompt_section_max_rules() {
        let mut rules = ProjectRules::default();
        for i in 0..25 {
            rules.coding_style.push(format!("Rule {i}"));
        }

        let prompt = rules.to_system_prompt_section("", 5);
        assert!(prompt.contains("20 lower-priority rules omitted"));
    }

    #[test]
    fn test_rules_loader_hot_reload() {
        let dir = tempdir().unwrap();
        let ownstack_dir = dir.path().join(".ownstack");
        fs::create_dir(&ownstack_dir).unwrap();
        fs::write(ownstack_dir.join("rules.md"), "# Forbidden\n- No eval\n")
            .unwrap();

        let mut loader = RulesLoader::new(dir.path().to_path_buf());
        let rules1 = loader.get_rules();
        assert_eq!(rules1.forbidden.len(), 1);

        // Modify file
        fs::write(
            ownstack_dir.join("rules.md"),
            "# Forbidden\n- No eval\n- No exec\n",
        )
        .unwrap();

        // Force mtime change (some filesystems have 1s granularity)
        loader.cached_mtime = None;
        let rules2 = loader.get_rules();
        assert_eq!(rules2.forbidden.len(), 2);
    }

    #[test]
    fn test_rules_loader_agents_md_priority() {
        let dir = tempdir().unwrap();
        let ownstack_dir = dir.path().join(".ownstack");
        fs::create_dir(&ownstack_dir).unwrap();
        fs::write(ownstack_dir.join("rules.md"), "# Style\n- Old rule\n").unwrap();
        fs::write(dir.path().join("AGENTS.md"), "# Style\n- New rule\n").unwrap();

        let mut loader = RulesLoader::new(dir.path().to_path_buf());
        let rules = loader.get_rules();

        // AGENTS.md takes priority
        assert_eq!(rules.coding_style, vec!["New rule"]);
    }

    #[test]
    fn test_is_empty() {
        let rules = ProjectRules::default();
        assert!(rules.is_empty());

        let mut rules2 = ProjectRules::default();
        rules2.forbidden.push("test".to_string());
        assert!(!rules2.is_empty());
    }

    #[test]
    fn test_to_system_prompt_uses_structured_parsing() {
        let dir = tempdir().unwrap();
        let ownstack_dir = dir.path().join(".ownstack");
        fs::create_dir(&ownstack_dir).unwrap();
        fs::write(
            ownstack_dir.join("rules.md"),
            "# Forbidden\n- Never use unwrap\n\n# Style\n- Use snake_case\n",
        )
        .unwrap();

        let memory = ProjectMemory::new(dir.path().to_path_buf());
        let prompt = memory.to_system_prompt();

        // Should use structured output with priority markers
        assert!(prompt.contains("🚨"));
        assert!(prompt.contains("FORBIDDEN"));
        assert!(prompt.contains("snake_case"));
    }
}
