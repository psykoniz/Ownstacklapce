use crate::toolkits::{ToolDef, ToolResult, Toolkit, ToolkitError};
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use serde_json::json;
use std::cmp::Reverse;
use std::fs;
use std::path::Path;

pub struct DocsToolkit;

#[derive(Deserialize)]
struct SearchExternalDocsArgs {
    query: String,
    max_results: Option<usize>,
    include_local: Option<bool>,
}

#[derive(Deserialize)]
struct GenerateDiagramArgs {
    description: String,
    diagram_type: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct ExternalDocHit {
    title: &'static str,
    url: &'static str,
    matched_keywords: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
struct LocalDocHit {
    file: String,
    line: usize,
    snippet: String,
}

#[derive(Debug, Clone, Copy)]
struct CuratedDoc {
    title: &'static str,
    url: &'static str,
    keywords: &'static [&'static str],
}

const CURATED_DOCS: &[CuratedDoc] = &[
    CuratedDoc {
        title: "Rust Book",
        url: "https://doc.rust-lang.org/book/",
        keywords: &["rust", "cargo", "ownership", "lifetime", "borrow"],
    },
    CuratedDoc {
        title: "Tokio Documentation",
        url: "https://docs.rs/tokio/latest/tokio/",
        keywords: &["tokio", "async", "await", "runtime"],
    },
    CuratedDoc {
        title: "Serde Documentation",
        url: "https://serde.rs/",
        keywords: &["serde", "serialize", "deserialize", "json"],
    },
    CuratedDoc {
        title: "Wasmtime Documentation",
        url: "https://docs.wasmtime.dev/",
        keywords: &["wasi", "wasm", "wasmtime", "plugin"],
    },
    CuratedDoc {
        title: "OpenRouter API Reference",
        url: "https://openrouter.ai/docs/api-reference/overview",
        keywords: &["openrouter", "llm", "chat", "stream"],
    },
    CuratedDoc {
        title: "Anthropic API Docs",
        url: "https://docs.anthropic.com/en/api/overview",
        keywords: &["anthropic", "claude", "messages", "tool_use"],
    },
    CuratedDoc {
        title: "Model Context Protocol",
        url: "https://modelcontextprotocol.io/docs/introduction",
        keywords: &["mcp", "model context protocol", "tool server"],
    },
    CuratedDoc {
        title: "Pytest Documentation",
        url: "https://docs.pytest.org/en/stable/",
        keywords: &["pytest", "python test", "fixture", "parametrize"],
    },
    CuratedDoc {
        title: "FastAPI Documentation",
        url: "https://fastapi.tiangolo.com/",
        keywords: &["fastapi", "pydantic", "python api"],
    },
    CuratedDoc {
        title: "GitHub Actions Documentation",
        url: "https://docs.github.com/actions",
        keywords: &["github actions", "workflow", "ci", "release"],
    },
];

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn should_skip_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    matches!(
        name,
        ".git" | "target" | "node_modules" | ".venv" | "venv" | "__pycache__"
    )
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| s.len() >= 2)
        .collect::<Vec<_>>()
}

fn curated_hits(query: &str, max_results: usize) -> Vec<ExternalDocHit> {
    let keywords = tokenize_query(query);
    if keywords.is_empty() {
        return Vec::new();
    }

    let mut scored = Vec::new();
    for item in CURATED_DOCS {
        let mut matched = Vec::new();
        for q in &keywords {
            if item.keywords.iter().any(|k| k.contains(q) || q.contains(k)) {
                matched.push(q.clone());
            }
        }
        if !matched.is_empty() {
            scored.push((matched.len(), matched, item));
        }
    }

    scored.sort_by_key(|(score, _, _)| Reverse(*score));
    scored
        .into_iter()
        .take(max_results)
        .map(|(_, matched, item)| ExternalDocHit {
            title: item.title,
            url: item.url,
            matched_keywords: matched,
        })
        .collect()
}

fn is_doc_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if lower.starts_with("readme") {
        return true;
    }
    if lower == "gemini.md" || lower == "architecture.md" {
        return true;
    }
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    matches!(ext.to_ascii_lowercase().as_str(), "md" | "txt" | "rst")
}

fn collect_local_doc_hits(
    root: &Path,
    query: &str,
    max_results: usize,
) -> Result<Vec<LocalDocHit>, ToolkitError> {
    if !root.exists() || !root.is_dir() {
        return Ok(Vec::new());
    }
    let keywords = tokenize_query(query);
    if keywords.is_empty() {
        return Ok(Vec::new());
    }

    fn walk(
        root: &Path,
        dir: &Path,
        keywords: &[String],
        max_results: usize,
        out: &mut Vec<LocalDocHit>,
    ) -> Result<(), ToolkitError> {
        if out.len() >= max_results {
            return Ok(());
        }
        let entries = fs::read_dir(dir).map_err(|e| {
            ToolkitError::ExecutionFailed(format!(
                "Failed to read directory {}: {}",
                dir.display(),
                e
            ))
        })?;
        for entry in entries {
            if out.len() >= max_results {
                break;
            }
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if should_skip_dir(&path) {
                    continue;
                }
                walk(root, &path, keywords, max_results, out)?;
                continue;
            }
            if !file_type.is_file() || !is_doc_candidate(&path) {
                continue;
            }

            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            if metadata.len() > 512 * 1024 {
                continue;
            }

            let content = match fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            for (idx, line) in content.lines().enumerate() {
                if out.len() >= max_results {
                    break;
                }
                let lower = line.to_ascii_lowercase();
                if keywords.iter().any(|k| lower.contains(k)) {
                    let file = match path.strip_prefix(root) {
                        Ok(relative) => normalize_path(relative),
                        Err(_) => normalize_path(&path),
                    };
                    out.push(LocalDocHit {
                        file,
                        line: idx + 1,
                        snippet: line.trim().to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    let mut hits = Vec::new();
    walk(root, root, &keywords, max_results, &mut hits)?;
    Ok(hits)
}

fn split_steps(description: &str) -> Vec<String> {
    let mut steps = description
        .replace("->", ".")
        .split(['\n', ';', '.'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
    if steps.is_empty() {
        steps.push("Start".to_string());
        steps.push("Process".to_string());
        steps.push("End".to_string());
    }
    steps.truncate(8);
    steps
}

fn infer_diagram_type(description: &str, explicit: Option<&str>) -> &'static str {
    if let Some(value) = explicit {
        let lower = value.to_ascii_lowercase();
        if lower.contains("sequence") {
            return "sequenceDiagram";
        }
        if lower.contains("flow") || lower.contains("graph") {
            return "flowchart";
        }
    }
    let lower = description.to_ascii_lowercase();
    if lower.contains("request")
        || lower.contains("response")
        || lower.contains("client")
        || lower.contains("server")
    {
        "sequenceDiagram"
    } else {
        "flowchart"
    }
}

fn label_for_node(text: &str) -> String {
    text.replace('"', "'")
        .replace('[', "(")
        .replace(']', ")")
        .trim()
        .to_string()
}

fn build_flowchart(steps: &[String], title: Option<&str>) -> String {
    let mut lines = vec!["graph TD".to_string()];
    if let Some(t) = title {
        if !t.trim().is_empty() {
            lines.push(format!("  %% {}", t.trim()));
        }
    }

    for (idx, step) in steps.iter().enumerate() {
        let current = format!("N{}", idx + 1);
        lines.push(format!("  {}[\"{}\"]", current, label_for_node(step)));
        if idx + 1 < steps.len() {
            let next = format!("N{}", idx + 2);
            lines.push(format!("  {} --> {}", current, next));
        }
    }
    lines.join("\n")
}

fn build_sequence(steps: &[String], title: Option<&str>) -> String {
    let mut lines = vec!["sequenceDiagram".to_string()];
    if let Some(t) = title {
        if !t.trim().is_empty() {
            lines.push(format!("  %% {}", t.trim()));
        }
    }
    lines.push("  participant User".to_string());
    lines.push("  participant Agent".to_string());
    lines.push("  participant System".to_string());

    for (idx, step) in steps.iter().enumerate() {
        let clean = label_for_node(step);
        let line = match idx % 3 {
            0 => format!("  User->>Agent: {}", clean),
            1 => format!("  Agent->>System: {}", clean),
            _ => format!("  System-->>Agent: {}", clean),
        };
        lines.push(line);
    }
    lines.join("\n")
}

#[async_trait]
impl Toolkit for DocsToolkit {
    fn name(&self) -> &str {
        "docs"
    }

    fn tools(&self) -> Vec<ToolDef> {
        vec![
            ToolDef {
                name: "search_external_docs".to_string(),
                description: "Search curated official documentation and local docs snippets."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "Search query (example: 'tokio stream')."
                        },
                        "max_results": {
                            "type": "integer",
                            "description": "Maximum results for external/local sections (default 8, max 30)."
                        },
                        "include_local": {
                            "type": "boolean",
                            "description": "Whether to include local docs matches (default true)."
                        }
                    },
                    "required": ["query"],
                }),
            },
            ToolDef {
                name: "generate_diagram".to_string(),
                description: "Generate Mermaid flowchart or sequence diagram from text description."
                    .to_string(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "description": {
                            "type": "string",
                            "description": "Narrative of the architecture/flow."
                        },
                        "diagram_type": {
                            "type": "string",
                            "description": "Optional override: 'flowchart' or 'sequence'."
                        },
                        "title": {
                            "type": "string",
                            "description": "Optional diagram title comment."
                        }
                    },
                    "required": ["description"],
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
            "search_external_docs" => {
                let parsed: SearchExternalDocsArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let query = parsed.query.trim();
                if query.is_empty() {
                    return Err(ToolkitError::InvalidArguments(
                        "query must not be empty".to_string(),
                    ));
                }
                let max_results = parsed.max_results.unwrap_or(8).clamp(1, 30);
                let include_local = parsed.include_local.unwrap_or(true);

                let external = curated_hits(query, max_results);
                let local = if include_local {
                    let cwd = std::env::current_dir().map_err(|e| {
                        ToolkitError::ExecutionFailed(format!(
                            "Failed to resolve current directory: {}",
                            e
                        ))
                    })?;
                    collect_local_doc_hits(&cwd, query, max_results)?
                } else {
                    Vec::new()
                };

                let response = json!({
                    "query": query,
                    "external_results_count": external.len(),
                    "external_results": external,
                    "local_results_count": local.len(),
                    "local_results": local,
                });
                Ok(ToolResult::success(response.to_string()))
            }
            "generate_diagram" => {
                let parsed: GenerateDiagramArgs = serde_json::from_value(args)
                    .map_err(|e| ToolkitError::InvalidArguments(e.to_string()))?;
                let description = parsed.description.trim();
                if description.is_empty() {
                    return Err(ToolkitError::InvalidArguments(
                        "description must not be empty".to_string(),
                    ));
                }

                let steps = split_steps(description);
                let kind =
                    infer_diagram_type(description, parsed.diagram_type.as_deref());
                let mermaid = if kind == "sequenceDiagram" {
                    build_sequence(&steps, parsed.title.as_deref())
                } else {
                    build_flowchart(&steps, parsed.title.as_deref())
                };

                let response = json!({
                    "diagram_type": if kind == "sequenceDiagram" { "sequence" } else { "flowchart" },
                    "steps_count": steps.len(),
                    "mermaid": mermaid,
                });
                Ok(ToolResult::success(response.to_string()))
            }
            _ => Err(ToolkitError::ToolNotFound(tool_name.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_query() {
        let tokens = tokenize_query("Tokio stream + Rust async");
        assert!(tokens.contains(&"tokio".to_string()));
        assert!(tokens.contains(&"stream".to_string()));
        assert!(tokens.contains(&"rust".to_string()));
    }

    #[test]
    fn curated_docs_match_keywords() {
        let hits = curated_hits("rust tokio async", 5);
        assert!(!hits.is_empty());
        assert!(hits.iter().any(|h| h.title.contains("Rust")));
    }

    #[test]
    fn generates_flowchart_mermaid() {
        let steps = split_steps("Open file. Analyze content. Save output.");
        let code = build_flowchart(&steps, Some("Flow"));
        assert!(code.starts_with("graph TD"));
        assert!(code.contains("-->"));
    }

    #[test]
    fn generates_sequence_mermaid() {
        let steps = split_steps("User request; Agent validates; System responds");
        let code = build_sequence(&steps, Some("Seq"));
        assert!(code.starts_with("sequenceDiagram"));
        assert!(code.contains("participant User"));
    }

    #[test]
    fn local_doc_search_finds_matches() {
        let temp = tempfile::tempdir().expect("tempdir");
        let docs_dir = temp.path().join("docs");
        fs::create_dir_all(&docs_dir).expect("mkdir docs");
        fs::write(docs_dir.join("guide.md"), "Tokio stream processing guide")
            .expect("write");

        let hits =
            collect_local_doc_hits(temp.path(), "tokio stream", 10).expect("hits");
        assert_eq!(hits.len(), 1);
        assert!(hits[0].file.ends_with("docs/guide.md"));
    }
}
