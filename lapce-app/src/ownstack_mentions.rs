//! `@`-mention parsing and expansion for the AI chat input.
//!
//! Users can pin explicit context into a message:
//! - `@file:src/main.rs` — inlines a file's contents
//! - `@folder:src` — inlines a one-level listing of a directory
//! - `@workspace` — inlines a compact workspace file tree
//!
//! Parsing is pure and unit-tested; resolution (disk I/O) is done by the
//! caller, which holds the workspace path. The user's visible message keeps the
//! original `@mention` text; only the prompt *sent to the agent* is expanded.

use std::path::Path;

/// A parsed mention found in the input text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mention {
    /// `@file:<path>`
    File(String),
    /// `@folder:<path>`
    Folder(String),
    /// `@workspace`
    Workspace,
}

/// Parse all mentions from `input`, in order of appearance and de-duplicated.
pub fn parse_mentions(input: &str) -> Vec<Mention> {
    let mut out: Vec<Mention> = Vec::new();
    // Split on whitespace; mentions are single tokens.
    for tok in input.split_whitespace() {
        let m = if let Some(rest) = tok.strip_prefix("@file:") {
            let p = trim_token(rest);
            (!p.is_empty()).then(|| Mention::File(p.to_string()))
        } else if let Some(rest) = tok.strip_prefix("@folder:") {
            let p = trim_token(rest);
            (!p.is_empty()).then(|| Mention::Folder(p.to_string()))
        } else if trim_token(tok) == "@workspace" {
            Some(Mention::Workspace)
        } else {
            None
        };
        if let Some(m) = m {
            if !out.contains(&m) {
                out.push(m);
            }
        }
    }
    out
}

/// Strip trailing punctuation that often clings to a mention token
/// (e.g. `@file:a.rs,` or `@workspace.`).
fn trim_token(s: &str) -> &str {
    s.trim_end_matches([',', '.', ';', ')', ']', '!', '?'])
}

/// True if the input contains at least one mention.
pub fn has_mentions(input: &str) -> bool {
    !parse_mentions(input).is_empty()
}

/// Resolve a `@file` mention against the workspace root, returning a formatted
/// context section or an error note. Reads are size-capped to avoid blowing the
/// context window.
pub fn resolve_file(workspace: &Path, rel: &str) -> String {
    let path = workspace.join(rel);
    match std::fs::read_to_string(&path) {
        Ok(content) => {
            let capped = cap_chars(&content, 8000);
            format!("### {rel}\n```\n{capped}\n```")
        }
        Err(e) => format!("### {rel}\n(could not read: {e})"),
    }
}

/// Resolve a `@folder` mention to a one-level listing.
pub fn resolve_folder(workspace: &Path, rel: &str) -> String {
    let path = workspace.join(rel);
    match std::fs::read_dir(&path) {
        Ok(entries) => {
            let mut names: Vec<String> = entries
                .flatten()
                .map(|e| {
                    let is_dir = e.path().is_dir();
                    let name = e.file_name().to_string_lossy().to_string();
                    if is_dir {
                        format!("{name}/")
                    } else {
                        name
                    }
                })
                .collect();
            names.sort();
            format!("### {rel}/ (listing)\n{}", names.join("\n"))
        }
        Err(e) => format!("### {rel}/\n(could not list: {e})"),
    }
}

/// Resolve `@workspace` to a compact recursive file tree (bounded).
pub fn resolve_workspace(workspace: &Path) -> String {
    let mut files: Vec<String> = Vec::new();
    collect_tree(workspace, workspace, &mut files, 0);
    files.sort();
    files.truncate(200);
    format!("### Workspace files\n{}", files.join("\n"))
}

fn collect_tree(root: &Path, dir: &Path, out: &mut Vec<String>, depth: usize) {
    if depth > 4 || out.len() > 200 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if matches!(name.as_str(), ".git" | "target" | "node_modules" | ".ownstack")
        {
            continue;
        }
        if path.is_dir() {
            collect_tree(root, &path, out, depth + 1);
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().to_string());
        }
    }
}

/// Combine the user's original prompt with resolved context sections.
pub fn build_prompt(original: &str, sections: &[String]) -> String {
    if sections.is_empty() {
        return original.to_string();
    }
    format!(
        "## Pinned context (@-mentions)\n\n{}\n\n## Request\n{}",
        sections.join("\n\n"),
        original
    )
}

fn cap_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push_str("\n… (truncated)");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_file_mention() {
        let m = parse_mentions("look at @file:src/main.rs please");
        assert_eq!(m, vec![Mention::File("src/main.rs".to_string())]);
    }

    #[test]
    fn parses_folder_and_workspace() {
        let m = parse_mentions("@folder:src and @workspace");
        assert_eq!(
            m,
            vec![Mention::Folder("src".to_string()), Mention::Workspace]
        );
    }

    #[test]
    fn dedups_repeated_mentions() {
        let m = parse_mentions("@file:a.rs @file:a.rs @file:a.rs");
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn trims_trailing_punctuation() {
        let m = parse_mentions("see @file:a.rs, and @workspace.");
        assert_eq!(
            m,
            vec![Mention::File("a.rs".to_string()), Mention::Workspace]
        );
    }

    #[test]
    fn no_mentions_when_plain_text() {
        assert!(!has_mentions("just a normal message"));
        assert!(parse_mentions("email me at a@b.com").is_empty());
    }

    #[test]
    fn empty_file_path_ignored() {
        assert!(parse_mentions("@file: nothing").is_empty());
    }

    #[test]
    fn build_prompt_without_sections_is_identity() {
        assert_eq!(build_prompt("hello", &[]), "hello");
    }

    #[test]
    fn build_prompt_wraps_sections() {
        let out = build_prompt("fix it", &["### a.rs\ncode".to_string()]);
        assert!(out.contains("Pinned context"));
        assert!(out.contains("### a.rs"));
        assert!(out.contains("## Request\nfix it"));
    }

    #[test]
    fn cap_chars_truncates_long_input() {
        let long = "x".repeat(10_000);
        let capped = cap_chars(&long, 100);
        assert!(capped.contains("truncated"));
        assert!(capped.chars().count() < 200);
    }

    #[test]
    fn resolve_file_reports_missing() {
        let tmp = std::env::temp_dir();
        let out = resolve_file(&tmp, "definitely_missing_file_xyz.rs");
        assert!(out.contains("could not read"));
    }
}
