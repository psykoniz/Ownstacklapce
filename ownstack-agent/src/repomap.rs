//! RepoMap v2 — AST-based codebase graph for context injection.
//!
//! Walks the workspace, extracts function/class definitions from source files,
//! and builds a compact textual summary that can be injected into the LLM prompt
//! for accurate code navigation without sending full files.
//!
//! This is a Rust port of `ownstack-python/app/utils/repomap_v2.py` using
//! regex-based symbol extraction (tree-sitter integration planned for Phase 11).

use regex::Regex;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

// ─── Symbol Types ───────────────────────────────────────────────

/// A code symbol extracted from source files.
#[derive(Debug, Clone)]
pub struct CodeSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub file: PathBuf,
    pub line: usize,
    /// Functions/methods called by this symbol (best-effort).
    pub calls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SymbolKind {
    Function,
    Method,
    Class,
    Struct,
    Trait,
    Enum,
    Const,
    Import,
}

impl std::fmt::Display for SymbolKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Function => write!(f, "fn"),
            Self::Method => write!(f, "method"),
            Self::Class => write!(f, "class"),
            Self::Struct => write!(f, "struct"),
            Self::Trait => write!(f, "trait"),
            Self::Enum => write!(f, "enum"),
            Self::Const => write!(f, "const"),
            Self::Import => write!(f, "use"),
        }
    }
}

// ─── Language Detection ─────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Language {
    Rust,
    Python,
    TypeScript,
    JavaScript,
    Go,
    Java,
    C,
    Cpp,
    Ruby,
    Php,
    Unknown,
}

fn detect_language(path: &Path) -> Language {
    match path.extension().and_then(|e| e.to_str()) {
        Some("rs") => Language::Rust,
        Some("py") => Language::Python,
        Some("ts") | Some("tsx") => Language::TypeScript,
        Some("js") | Some("jsx") => Language::JavaScript,
        Some("go") => Language::Go,
        Some("java") => Language::Java,
        Some("c") | Some("h") => Language::C,
        Some("cpp") | Some("cc") | Some("cxx") | Some("hpp") | Some("hxx") => {
            Language::Cpp
        }
        Some("rb") => Language::Ruby,
        Some("php") => Language::Php,
        _ => Language::Unknown,
    }
}

fn compile_regex(pattern: &str, label: &str) -> Option<Regex> {
    match Regex::new(pattern) {
        Ok(regex) => Some(regex),
        Err(err) => {
            warn!(
                "RepoMap: failed to compile '{}' regex '{}': {}",
                label, pattern, err
            );
            None
        }
    }
}

// ─── Regex-based Symbol Extraction ──────────────────────────────

/// Extract symbols from Rust source code.
fn extract_rust_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (
        Some(fn_re),
        Some(struct_re),
        Some(enum_re),
        Some(trait_re),
        Some(impl_re),
        Some(call_re),
    ) = (
        compile_regex(
            r"(?m)^\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)",
            "rust_function",
        ),
        compile_regex(r"(?m)^\s*(?:pub\s+)?struct\s+(\w+)", "rust_struct"),
        compile_regex(r"(?m)^\s*(?:pub\s+)?enum\s+(\w+)", "rust_enum"),
        compile_regex(r"(?m)^\s*(?:pub\s+)?trait\s+(\w+)", "rust_trait"),
        compile_regex(r"(?m)^\s*impl(?:\s*<[^>]*>)?\s+(\w+)", "rust_impl"),
        compile_regex(r"(\w+)\s*\(", "rust_call"),
    )
    else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = fn_re.captures(line) {
            let name = cap[1].to_string();
            // Extract calls from the function body (simplified: next 20 lines)
            let body: String = content
                .lines()
                .skip(i + 1)
                .take(20)
                .collect::<Vec<_>>()
                .join("\n");
            let calls: Vec<String> = call_re
                .captures_iter(&body)
                .map(|c| c[1].to_string())
                .filter(|n| n != &name && !is_rust_keyword(n))
                .collect();

            symbols.push(CodeSymbol {
                name,
                kind: SymbolKind::Function,
                file: file.to_path_buf(),
                line: i + 1,
                calls,
            });
        }
        if let Some(cap) = struct_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Struct,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = enum_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Enum,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = trait_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Trait,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = impl_re.captures(line) {
            // Don't add impl as a symbol, but mark subsequent fn's as methods
            let _impl_name = &cap[1];
        }
    }

    symbols
}

/// Extract symbols from Python source code.
fn extract_python_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(fn_re), Some(class_re)) = (
        compile_regex(r"(?m)^(?:\s*)(?:async\s+)?def\s+(\w+)", "python_function"),
        compile_regex(r"(?m)^class\s+(\w+)", "python_class"),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = class_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Class,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = fn_re.captures(line) {
            let name = cap[1].to_string();
            let kind = if line.starts_with("    ") || line.starts_with('\t') {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            symbols.push(CodeSymbol {
                name,
                kind,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from TypeScript/JavaScript source code.
fn extract_ts_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(fn_re), Some(class_re), Some(const_fn_re)) = (
        compile_regex(
            r"(?m)^\s*(?:export\s+)?(?:async\s+)?function\s+(\w+)",
            "ts_function",
        ),
        compile_regex(r"(?m)^\s*(?:export\s+)?class\s+(\w+)", "ts_class"),
        compile_regex(
            r"(?m)^\s*(?:export\s+)?const\s+(\w+)\s*=\s*(?:async\s+)?\(",
            "ts_const_function",
        ),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = fn_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Function,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = class_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Class,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = const_fn_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Function,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from Go source code.
fn extract_go_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(fn_re), Some(type_re), Some(iface_re)) = (
        compile_regex(
            r"(?m)^func\s+(?:\(\s*\w+\s+\*?\w+\s*\)\s+)?(\w+)",
            "go_function",
        ),
        compile_regex(r"(?m)^type\s+(\w+)\s+struct\b", "go_struct"),
        compile_regex(r"(?m)^type\s+(\w+)\s+interface\b", "go_interface"),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = fn_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Function,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = type_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Struct,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = iface_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Trait,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from Java source code.
fn extract_java_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(class_re), Some(iface_re), Some(method_re)) = (
        compile_regex(
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:abstract\s+|final\s+)?class\s+(\w+)",
            "java_class",
        ),
        compile_regex(
            r"(?m)^\s*(?:public\s+)?interface\s+(\w+)",
            "java_interface",
        ),
        compile_regex(
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+)?(?:final\s+)?(?:abstract\s+)?(?:synchronized\s+)?(?:\w+(?:<[^>]*>)?(?:\[\])*)\s+(\w+)\s*\(",
            "java_method",
        ),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = class_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Class,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = iface_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Trait,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = method_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Method,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from C source code.
fn extract_c_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(fn_re), Some(struct_re), Some(typedef_re)) = (
        compile_regex(
            r"(?m)^(?:static\s+|extern\s+|inline\s+)*(?:(?:unsigned|signed|const|volatile|struct|enum)\s+)*\w+[\s*]+(\w+)\s*\([^;]*\)\s*\{",
            "c_function",
        ),
        compile_regex(r"(?m)^\s*(?:typedef\s+)?struct\s+(\w+)", "c_struct"),
        compile_regex(r"(?m)^\s*typedef\s+\w+\s+(\w+)\s*;", "c_typedef"),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = fn_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Function,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = struct_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Struct,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = typedef_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Struct,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from C++ source code.
fn extract_cpp_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(class_re), Some(fn_re), Some(ns_re)) = (
        compile_regex(
            r"(?m)^\s*(?:template\s*<[^>]*>\s*)?class\s+(\w+)",
            "cpp_class",
        ),
        compile_regex(
            r"(?m)^(?:static\s+|virtual\s+|inline\s+|explicit\s+|constexpr\s+)*(?:(?:unsigned|signed|const|volatile|struct|enum)\s+)*\w+[\s*&]+(?:\w+::)?(\w+)\s*\([^;]*\)\s*(?:const\s*)?(?:override\s*)?(?:noexcept\s*)?\{",
            "cpp_function",
        ),
        compile_regex(r"(?m)^namespace\s+(\w+)", "cpp_namespace"),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = class_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Class,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = fn_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Function,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = ns_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Enum,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from Ruby source code.
fn extract_ruby_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(class_re), Some(module_re), Some(def_re)) = (
        compile_regex(r"(?m)^\s*class\s+(\w+)", "ruby_class"),
        compile_regex(r"(?m)^\s*module\s+(\w+)", "ruby_module"),
        compile_regex(r"(?m)^\s*def\s+(?:self\.)?(\w+[!?]?)", "ruby_method"),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = class_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Class,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = module_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Enum,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = def_re.captures(line) {
            let kind = if line.starts_with("    ") || line.starts_with('\t') {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

/// Extract symbols from PHP source code.
fn extract_php_symbols(content: &str, file: &Path) -> Vec<CodeSymbol> {
    let mut symbols = Vec::new();

    let (Some(class_re), Some(fn_re), Some(iface_re)) = (
        compile_regex(
            r"(?m)^\s*(?:abstract\s+|final\s+)?class\s+(\w+)",
            "php_class",
        ),
        compile_regex(
            r"(?m)^\s*(?:public\s+|private\s+|protected\s+)?(?:static\s+)?function\s+(\w+)",
            "php_function",
        ),
        compile_regex(r"(?m)^\s*interface\s+(\w+)", "php_interface"),
    ) else {
        return symbols;
    };

    for (i, line) in content.lines().enumerate() {
        if let Some(cap) = class_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Class,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = fn_re.captures(line) {
            let kind = if line.starts_with("    ") || line.starts_with('\t') {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
        if let Some(cap) = iface_re.captures(line) {
            symbols.push(CodeSymbol {
                name: cap[1].to_string(),
                kind: SymbolKind::Trait,
                file: file.to_path_buf(),
                line: i + 1,
                calls: Vec::new(),
            });
        }
    }

    symbols
}

fn is_rust_keyword(name: &str) -> bool {
    matches!(
        name,
        "if" | "else"
            | "let"
            | "mut"
            | "for"
            | "while"
            | "loop"
            | "match"
            | "Some"
            | "None"
            | "Ok"
            | "Err"
            | "vec"
            | "format"
            | "println"
            | "eprintln"
            | "write"
            | "writeln"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "debug"
            | "info"
            | "warn"
            | "error"
            | "trace"
            | "String"
            | "Vec"
            | "Box"
            | "Arc"
            | "Rc"
            | "HashMap"
            | "HashSet"
    )
}

// ─── Skip Logic ─────────────────────────────────────────────────

const SKIP_DIRS: &[&str] = &[
    "target",
    "node_modules",
    ".git",
    "__pycache__",
    ".ownstack",
    "dist",
    "build",
    ".next",
    "venv",
    ".venv",
    "vendor",
];

const SKIP_FILES: &[&str] = &[
    "package-lock.json",
    "Cargo.lock",
    "yarn.lock",
    "pnpm-lock.yaml",
];

fn should_skip(path: &Path, workspace: &Path) -> bool {
    let relative = path.strip_prefix(workspace).unwrap_or(path);
    for component in relative.components() {
        let name = component.as_os_str().to_string_lossy();
        if SKIP_DIRS.iter().any(|d| *d == name.as_ref()) {
            return true;
        }
    }
    if let Some(filename) = path.file_name().and_then(|f| f.to_str()) {
        if SKIP_FILES.contains(&filename) {
            return true;
        }
    }
    false
}

// ─── RepoMap Builder ────────────────────────────────────────────

/// Walks the workspace and builds a symbol map.
pub struct RepoMap {
    workspace: PathBuf,
    symbols: Vec<CodeSymbol>,
    file_count: usize,
}

impl RepoMap {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            workspace,
            symbols: Vec::new(),
            file_count: 0,
        }
    }

    /// Scan the workspace and extract all symbols.
    pub fn scan(&mut self) -> &[CodeSymbol] {
        self.symbols.clear();
        self.file_count = 0;

        self.walk_dir(&self.workspace.clone());

        info!(
            "RepoMap: scanned {} files, found {} symbols",
            self.file_count,
            self.symbols.len()
        );

        &self.symbols
    }

    fn walk_dir(&mut self, dir: &Path) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if should_skip(&path, &self.workspace) {
                continue;
            }

            if path.is_dir() {
                self.walk_dir(&path);
            } else if path.is_file() {
                let lang = detect_language(&path);
                if lang == Language::Unknown {
                    continue;
                }

                if let Ok(content) = std::fs::read_to_string(&path) {
                    self.file_count += 1;
                    let new_symbols = match lang {
                        Language::Rust => extract_rust_symbols(&content, &path),
                        Language::Python => extract_python_symbols(&content, &path),
                        Language::TypeScript | Language::JavaScript => {
                            extract_ts_symbols(&content, &path)
                        }
                        Language::Go => extract_go_symbols(&content, &path),
                        Language::Java => extract_java_symbols(&content, &path),
                        Language::C => extract_c_symbols(&content, &path),
                        Language::Cpp => extract_cpp_symbols(&content, &path),
                        Language::Ruby => extract_ruby_symbols(&content, &path),
                        Language::Php => extract_php_symbols(&content, &path),
                        Language::Unknown => Vec::new(),
                    };
                    self.symbols.extend(new_symbols);
                }
            }
        }
    }

    /// Get all symbols.
    pub fn symbols(&self) -> &[CodeSymbol] {
        &self.symbols
    }

    /// Build a call graph: symbol_name → [called_symbols].
    pub fn call_graph(&self) -> HashMap<String, Vec<String>> {
        let mut graph: HashMap<String, Vec<String>> = HashMap::new();
        let known_names: std::collections::HashSet<String> =
            self.symbols.iter().map(|s| s.name.clone()).collect();

        for sym in &self.symbols {
            let valid_calls: Vec<String> = sym
                .calls
                .iter()
                .filter(|c| known_names.contains(*c))
                .cloned()
                .collect();
            if !valid_calls.is_empty() {
                graph.insert(sym.name.clone(), valid_calls);
            }
        }

        graph
    }

    /// Generate a compact text summary for LLM prompt injection.
    ///
    /// Format: `path/to/file.rs:42  fn  function_name → [called1, called2]`
    ///
    /// `max_lines` caps the number of symbol lines emitted.
    /// `max_chars` caps total output length — whichever limit is hit first
    /// stops emission. This prevents oversized system prompts that trigger
    /// 503s on providers with payload size limits.
    pub fn to_prompt_text(&self, max_lines: usize) -> String {
        self.to_prompt_text_budget(max_lines, Self::DEFAULT_MAX_CHARS)
    }

    const DEFAULT_MAX_CHARS: usize = 6000;

    pub fn to_prompt_text_budget(
        &self,
        max_lines: usize,
        max_chars: usize,
    ) -> String {
        let mut lines = Vec::new();
        let mut total_chars: usize = 0;

        for sym in &self.symbols {
            let relative =
                sym.file.strip_prefix(&self.workspace).unwrap_or(&sym.file);

            let calls_str = if sym.calls.is_empty() {
                String::new()
            } else {
                format!(" → [{}]", sym.calls.join(", "))
            };

            let line = format!(
                "{}:{}  {}  {}{}",
                relative.display(),
                sym.line,
                sym.kind,
                sym.name,
                calls_str
            );

            total_chars += line.len() + 1;
            if total_chars > max_chars || lines.len() >= max_lines {
                lines.push(format!(
                    "... ({} more symbols)",
                    self.symbols.len() - lines.len()
                ));
                break;
            }

            lines.push(line);
        }

        if lines.is_empty() {
            return String::new();
        }

        format!(
            "## Repository Map ({} symbols)\n\n{}",
            self.symbols.len(),
            lines.join("\n")
        )
    }

    /// Get symbols relevant to specific keywords (for context-aware injection).
    pub fn relevant_symbols(&self, keywords: &[&str]) -> Vec<&CodeSymbol> {
        self.symbols
            .iter()
            .filter(|sym| {
                let name_lower = sym.name.to_lowercase();
                let path_lower = sym.file.to_string_lossy().to_lowercase();
                keywords.iter().any(|kw| {
                    name_lower.contains(&kw.to_lowercase())
                        || path_lower.contains(&kw.to_lowercase())
                })
            })
            .collect()
    }
}

// ─── Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_extract_rust_symbols() {
        let content = r#"
pub struct Config {
    pub name: String,
}

pub fn load_config(path: &str) -> Config {
    let data = read_file(path);
    parse_toml(data)
}

async fn process() {
    load_config("test");
}

pub enum Status {
    Active,
    Inactive,
}

pub trait Handler {
    fn handle(&self);
}
"#;
        let symbols = extract_rust_symbols(content, Path::new("test.rs"));
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();

        assert!(names.contains(&"Config"));
        assert!(names.contains(&"load_config"));
        assert!(names.contains(&"process"));
        assert!(names.contains(&"Status"));
        assert!(names.contains(&"Handler"));
    }

    #[test]
    fn test_extract_python_symbols() {
        let content = r#"
class Agent:
    def __init__(self):
        pass

    async def run(self):
        pass

def main():
    agent = Agent()
"#;
        let symbols = extract_python_symbols(content, Path::new("agent.py"));
        let names: Vec<&str> = symbols.iter().map(|s| s.name.as_str()).collect();

        assert!(names.contains(&"Agent"));
        assert!(names.contains(&"__init__"));
        assert!(names.contains(&"run"));
        assert!(names.contains(&"main"));
    }

    #[test]
    fn test_extract_ts_symbols() {
        let content = r#"
export class UserService {
    async getUser(id: string) {}
}

export function createApp() {}

export const handleRequest = async (req: Request) => {};
"#;
        let symbols = extract_ts_symbols(content, Path::new("app.ts"));

        assert!(symbols.iter().any(|s| s.name == "UserService"));
        assert!(symbols.iter().any(|s| s.name == "createApp"));
        assert!(symbols.iter().any(|s| s.name == "handleRequest"));
    }

    #[test]
    fn test_rust_call_extraction() {
        let content = r#"
fn helper() {}

fn main() {
    helper();
    let x = compute(42);
}

fn compute(n: i32) -> i32 { n * 2 }
"#;
        let symbols = extract_rust_symbols(content, Path::new("test.rs"));
        let main_sym = symbols.iter().find(|s| s.name == "main").unwrap();

        assert!(main_sym.calls.contains(&"helper".to_string()));
        assert!(main_sym.calls.contains(&"compute".to_string()));
    }

    #[test]
    fn test_repomap_scan() {
        let dir = tempdir().unwrap();
        let src_dir = dir.path().join("src");
        fs::create_dir(&src_dir).unwrap();

        fs::write(
            src_dir.join("lib.rs"),
            "pub fn greet() {}\npub struct App {}\n",
        )
        .unwrap();

        fs::write(
            src_dir.join("utils.py"),
            "def helper():\n    pass\n\nclass Tool:\n    pass\n",
        )
        .unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();

        assert!(map.symbols().len() >= 4); // greet, App, helper, Tool
        assert!(map.file_count >= 2);
    }

    #[test]
    fn test_repomap_prompt_text() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "pub fn entry_point() {}\npub struct Server {}\n",
        )
        .unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();

        let txt = map.to_prompt_text(100);
        assert!(txt.contains("Repository Map"));
        assert!(txt.contains("entry_point"));
        assert!(txt.contains("Server"));
    }

    #[test]
    fn test_relevant_symbols() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("auth.rs"),
            "pub fn login() {}\npub fn logout() {}\npub fn render() {}\n",
        )
        .unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();

        let relevant = map.relevant_symbols(&["auth", "login"]);
        // Should match by file path "auth" and name "login"
        assert!(!relevant.is_empty());
    }

    #[test]
    fn test_skip_dirs() {
        let dir = tempdir().unwrap();
        let target_dir = dir.path().join("target").join("debug");
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(target_dir.join("main.rs"), "fn hidden() {}").unwrap();

        fs::write(dir.path().join("src.rs"), "fn visible() {}").unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();

        let names: Vec<&str> =
            map.symbols().iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"visible"));
        assert!(!names.contains(&"hidden"));
    }

    #[test]
    fn test_call_graph() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join("main.rs"),
            "fn helper() {}\nfn main() {\n    helper();\n}\n",
        )
        .unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();

        let graph = map.call_graph();
        assert!(graph
            .get("main")
            .map_or(false, |calls| calls.contains(&"helper".to_string())));
    }

    #[test]
    fn char_budget_caps_output() {
        let dir = tempdir().unwrap();
        let mut src = String::new();
        for i in 0..100 {
            src.push_str(&format!("pub fn function_number_{}() {{}}\n", i));
        }
        fs::write(dir.path().join("big.rs"), &src).unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();
        assert!(map.symbols().len() >= 100);

        let uncapped = map.to_prompt_text_budget(200, 100_000);
        let capped = map.to_prompt_text_budget(200, 500);

        assert!(capped.len() < uncapped.len());
        assert!(capped.len() <= 600);
        assert!(capped.contains("more symbols"));
    }

    #[test]
    fn default_to_prompt_text_respects_char_limit() {
        let dir = tempdir().unwrap();
        let mut src = String::new();
        for i in 0..500 {
            src.push_str(&format!(
                "pub fn very_long_function_name_number_{}() {{}}\n",
                i
            ));
        }
        fs::write(dir.path().join("huge.rs"), &src).unwrap();

        let mut map = RepoMap::new(dir.path().to_path_buf());
        map.scan();

        let txt = map.to_prompt_text(500);
        assert!(
            txt.len() <= RepoMap::DEFAULT_MAX_CHARS + 200,
            "default to_prompt_text should stay near the char budget, got {} chars",
            txt.len()
        );
    }

    #[test]
    fn test_extract_go_symbols() {
        let src = r#"
package main

func main() {}
func (s *Server) Handle() {}

type Config struct {
    Port int
}

type Handler interface {
    ServeHTTP()
}
"#;
        let syms = extract_go_symbols(src, Path::new("main.go"));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"Handle"));
        assert!(names.contains(&"Config"));
        assert!(names.contains(&"Handler"));
    }

    #[test]
    fn test_extract_java_symbols() {
        let src = r#"
public class UserService {
    public void createUser(String name) {}
    private int calculateAge(int year) { return 0; }
}

public interface Repository {
    void save();
}
"#;
        let syms = extract_java_symbols(src, Path::new("UserService.java"));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"UserService"));
        assert!(names.contains(&"createUser"));
        assert!(names.contains(&"Repository"));
    }

    #[test]
    fn test_extract_ruby_symbols() {
        let src = r#"
class ApplicationController
  def index
    render json: {}
  end

  def self.configure
  end
end

module Helpers
  def format_date(date)
  end
end
"#;
        let syms = extract_ruby_symbols(src, Path::new("app.rb"));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"ApplicationController"));
        assert!(names.contains(&"index"));
        assert!(names.contains(&"Helpers"));
        assert!(names.contains(&"format_date"));
    }

    #[test]
    fn test_extract_php_symbols() {
        let src = r#"<?php
class UserController {
    public function index() {}
    private static function validate() {}
}

interface Cacheable {
    public function cache();
}
"#;
        let syms = extract_php_symbols(src, Path::new("UserController.php"));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"UserController"));
        assert!(names.contains(&"index"));
        assert!(names.contains(&"Cacheable"));
    }

    #[test]
    fn test_extract_c_symbols() {
        let src = r#"
typedef struct Config Config;

struct Server {
    int port;
};

int main(int argc, char *argv[]) {
    return 0;
}

static void helper(void) {
}
"#;
        let syms = extract_c_symbols(src, Path::new("main.c"));
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Server"));
        assert!(names.contains(&"main"));
    }

    #[test]
    fn test_detect_new_languages() {
        assert_eq!(detect_language(Path::new("main.go")), Language::Go);
        assert_eq!(detect_language(Path::new("App.java")), Language::Java);
        assert_eq!(detect_language(Path::new("main.c")), Language::C);
        assert_eq!(detect_language(Path::new("util.h")), Language::C);
        assert_eq!(detect_language(Path::new("app.cpp")), Language::Cpp);
        assert_eq!(detect_language(Path::new("app.cc")), Language::Cpp);
        assert_eq!(detect_language(Path::new("app.rb")), Language::Ruby);
        assert_eq!(detect_language(Path::new("index.php")), Language::Php);
        assert_eq!(detect_language(Path::new("unknown.xyz")), Language::Unknown);
    }
}
