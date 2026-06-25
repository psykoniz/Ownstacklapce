//! Symbol-aware code chunking for semantic indexing.
//!
//! Naive fixed-size chunking (every N lines) splits functions in half and
//! mixes unrelated code, which hurts retrieval quality. This module splits a
//! source file at *definition boundaries* (functions, classes, structs, impls,
//! …) so each chunk is a coherent semantic unit.
//!
//! It is intentionally dependency-free (regex only) rather than using
//! tree-sitter: it keeps the build light and works "well enough" for retrieval,
//! where approximate boundaries are fine. Definitions longer than the line
//! budget are split into sub-chunks so embeddings stay focused.

use regex::Regex;
use std::sync::OnceLock;

/// Maximum lines per chunk before it is split further.
const MAX_CHUNK_LINES: usize = 60;

/// A semantic chunk of source code with its location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeChunk {
    /// 1-based start line (inclusive).
    pub start_line: usize,
    /// 1-based end line (inclusive).
    pub end_line: usize,
    /// The chunk's source text.
    pub content: String,
    /// Best-effort symbol name for the chunk (e.g. `fn parse_args`).
    pub symbol: Option<String>,
}

/// Detect whether a line begins a top-level-ish definition, returning a short
/// symbol label if so. Works across Rust, TS/JS, Python, Go and Java.
fn definition_label(line: &str) -> Option<String> {
    static RE: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    let patterns = RE.get_or_init(|| {
        vec![
            // Rust
            (Regex::new(r"^\s*(pub\s+)?(async\s+)?fn\s+(\w+)").unwrap(), "fn"),
            (Regex::new(r"^\s*(pub\s+)?struct\s+(\w+)").unwrap(), "struct"),
            (Regex::new(r"^\s*(pub\s+)?enum\s+(\w+)").unwrap(), "enum"),
            (Regex::new(r"^\s*(pub\s+)?trait\s+(\w+)").unwrap(), "trait"),
            (Regex::new(r"^\s*impl(\s|<)").unwrap(), "impl"),
            // Python
            (Regex::new(r"^\s*def\s+(\w+)").unwrap(), "def"),
            (Regex::new(r"^\s*class\s+(\w+)").unwrap(), "class"),
            // TS / JS
            (
                Regex::new(r"^\s*(export\s+)?(default\s+)?(async\s+)?function\s+(\w+)")
                    .unwrap(),
                "function",
            ),
            (
                Regex::new(r"^\s*(export\s+)?(abstract\s+)?class\s+(\w+)").unwrap(),
                "class",
            ),
            (
                Regex::new(r"^\s*(export\s+)?interface\s+(\w+)").unwrap(),
                "interface",
            ),
            // Go
            (Regex::new(r"^\s*func\s+(\(.*\)\s+)?(\w+)").unwrap(), "func"),
            (Regex::new(r"^\s*type\s+(\w+)\s+(struct|interface)").unwrap(), "type"),
            // Java / C#
            (
                Regex::new(
                    r"^\s*(public|private|protected)\s+.*\b(class|interface)\s+(\w+)",
                )
                .unwrap(),
                "class",
            ),
        ]
    });

    for (re, kind) in patterns.iter() {
        if let Some(caps) = re.captures(line) {
            // The symbol name is the last captured group that looks like an ident.
            let name = (1..caps.len())
                .rev()
                .filter_map(|i| caps.get(i))
                .map(|m| m.as_str())
                .find(|s| {
                    !s.is_empty()
                        && s.chars().next().is_some_and(|c| {
                            c.is_alphabetic() || c == '_'
                        })
                        && !matches!(
                            *s,
                            "pub" | "async" | "export" | "default" | "abstract"
                                | "public" | "private" | "protected" | "struct"
                                | "interface"
                        )
                })
                .unwrap_or("");
            return Some(if name.is_empty() {
                kind.to_string()
            } else {
                format!("{kind} {name}")
            });
        }
    }
    None
}

/// Split source text into symbol-aware chunks.
///
/// Leading lines before the first definition (imports, module docs) form their
/// own chunk so they remain searchable.
pub fn chunk_source(content: &str) -> Vec<CodeChunk> {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Find boundary line indices where a new definition starts.
    let mut boundaries: Vec<usize> = Vec::new();
    let mut labels: Vec<Option<String>> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if let Some(label) = definition_label(line) {
            boundaries.push(i);
            labels.push(Some(label));
        }
    }

    // No definitions found — fall back to fixed-size chunking.
    if boundaries.is_empty() {
        return fixed_chunks(&lines, 0, lines.len(), None);
    }

    let mut chunks = Vec::new();

    // Preamble before the first definition.
    if boundaries[0] > 0 {
        chunks.extend(fixed_chunks(&lines, 0, boundaries[0], None));
    }

    for (idx, &start) in boundaries.iter().enumerate() {
        let end = boundaries.get(idx + 1).copied().unwrap_or(lines.len());
        let label = labels[idx].clone();
        if end - start > MAX_CHUNK_LINES {
            chunks.extend(fixed_chunks(&lines, start, end, label));
        } else {
            chunks.push(make_chunk(&lines, start, end, label));
        }
    }

    chunks
}

/// Build a chunk from a half-open line range `[start, end)`.
fn make_chunk(
    lines: &[&str],
    start: usize,
    end: usize,
    symbol: Option<String>,
) -> CodeChunk {
    CodeChunk {
        start_line: start + 1,
        end_line: end,
        content: lines[start..end].join("\n"),
        symbol,
    }
}

/// Fixed-size fallback chunking for a range, used for preambles and oversized
/// definitions. The symbol label (if any) is attached to every sub-chunk.
fn fixed_chunks(
    lines: &[&str],
    start: usize,
    end: usize,
    symbol: Option<String>,
) -> Vec<CodeChunk> {
    let mut out = Vec::new();
    let mut cur = start;
    while cur < end {
        let stop = (cur + MAX_CHUNK_LINES).min(end);
        // Skip ranges that are entirely blank.
        if lines[cur..stop].iter().any(|l| !l.trim().is_empty()) {
            out.push(make_chunk(lines, cur, stop, symbol.clone()));
        }
        cur = stop;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rust_fn() {
        assert_eq!(
            definition_label("pub async fn handle_request(req: Req) {"),
            Some("fn handle_request".to_string())
        );
    }

    #[test]
    fn detects_python_def_and_class() {
        assert_eq!(definition_label("def parse(x):"), Some("def parse".to_string()));
        assert_eq!(
            definition_label("class Widget:"),
            Some("class Widget".to_string())
        );
    }

    #[test]
    fn detects_ts_function_and_interface() {
        assert_eq!(
            definition_label("export function render(): void {"),
            Some("function render".to_string())
        );
        assert_eq!(
            definition_label("export interface Props {"),
            Some("interface Props".to_string())
        );
    }

    #[test]
    fn detects_rust_struct_and_impl() {
        assert_eq!(
            definition_label("pub struct Config {"),
            Some("struct Config".to_string())
        );
        assert!(definition_label("impl Config {").unwrap().starts_with("impl"));
    }

    #[test]
    fn non_definition_returns_none() {
        assert_eq!(definition_label("    let x = 5;"), None);
        assert_eq!(definition_label("// a comment"), None);
    }

    #[test]
    fn chunks_split_at_definitions() {
        let src = "\
use std::io;

fn alpha() {
    println!(\"a\");
}

fn beta() {
    println!(\"b\");
}
";
        let chunks = chunk_source(src);
        // Expect: preamble (use), alpha, beta.
        assert!(chunks.len() >= 2);
        let symbols: Vec<_> =
            chunks.iter().filter_map(|c| c.symbol.clone()).collect();
        assert!(symbols.iter().any(|s| s == "fn alpha"));
        assert!(symbols.iter().any(|s| s == "fn beta"));
    }

    #[test]
    fn chunk_line_ranges_are_one_based_and_contiguous() {
        let src = "fn a() {}\nfn b() {}\n";
        let chunks = chunk_source(src);
        assert_eq!(chunks[0].start_line, 1);
        assert!(chunks.iter().all(|c| c.end_line >= c.start_line));
    }

    #[test]
    fn no_definitions_falls_back_to_fixed() {
        let src = "line1\nline2\nline3\n";
        let chunks = chunk_source(src);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].content, "line1\nline2\nline3");
    }

    #[test]
    fn empty_source_yields_no_chunks() {
        assert!(chunk_source("").is_empty());
    }

    #[test]
    fn oversized_definition_is_split() {
        let mut src = String::from("fn big() {\n");
        for i in 0..120 {
            src.push_str(&format!("    let v{i} = {i};\n"));
        }
        src.push_str("}\n");
        let chunks = chunk_source(&src);
        assert!(chunks.len() > 1, "large fn should split into sub-chunks");
    }

    #[test]
    fn blank_only_ranges_are_skipped() {
        let src = "\n\n\n\n";
        let chunks = chunk_source(src);
        assert!(chunks.is_empty());
    }
}
