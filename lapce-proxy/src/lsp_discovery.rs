use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

/// Describes a known LSP server for a given language.
pub struct LspEntry {
    pub language_id: &'static str,
    pub file_extensions: &'static [&'static str],
    pub binary_names: &'static [&'static str],
    pub install_hint: &'static str,
}

/// Result of attempting to detect an LSP server.
pub enum LspDetectionResult {
    Found {
        binary: String,
        language_id: String,
    },
    NotInstalled {
        language_id: String,
        install_hint: String,
    },
}

/// Registry of well-known LSP servers for common languages.
pub static LSP_REGISTRY: &[LspEntry] = &[
    LspEntry {
        language_id: "rust",
        file_extensions: &["rs"],
        binary_names: &["rust-analyzer"],
        install_hint: "rustup component add rust-analyzer",
    },
    LspEntry {
        language_id: "python",
        file_extensions: &["py", "pyi"],
        binary_names: &["pyright", "pylsp", "python-language-server"],
        install_hint: "pip install pyright",
    },
    LspEntry {
        language_id: "typescript",
        file_extensions: &["ts", "tsx"],
        binary_names: &["typescript-language-server"],
        install_hint: "npm i -g typescript-language-server typescript",
    },
    LspEntry {
        language_id: "javascript",
        file_extensions: &["js", "jsx", "mjs", "cjs"],
        binary_names: &["typescript-language-server"],
        install_hint: "npm i -g typescript-language-server typescript",
    },
    LspEntry {
        language_id: "go",
        file_extensions: &["go"],
        binary_names: &["gopls"],
        install_hint: "go install golang.org/x/tools/gopls@latest",
    },
    LspEntry {
        language_id: "c",
        file_extensions: &["c", "h"],
        binary_names: &["clangd"],
        install_hint: "apt install clangd or brew install llvm",
    },
    LspEntry {
        language_id: "cpp",
        file_extensions: &["cpp", "cxx", "cc", "hpp", "hxx"],
        binary_names: &["clangd"],
        install_hint: "apt install clangd or brew install llvm",
    },
    LspEntry {
        language_id: "java",
        file_extensions: &["java"],
        binary_names: &["jdtls"],
        install_hint: "install Eclipse JDT Language Server",
    },
    LspEntry {
        language_id: "ruby",
        file_extensions: &["rb"],
        binary_names: &["solargraph"],
        install_hint: "gem install solargraph",
    },
    LspEntry {
        language_id: "php",
        file_extensions: &["php"],
        binary_names: &["phpactor", "intelephense"],
        install_hint: "npm i -g intelephense",
    },
    LspEntry {
        language_id: "lua",
        file_extensions: &["lua"],
        binary_names: &["lua-language-server"],
        install_hint: "brew install lua-language-server",
    },
    LspEntry {
        language_id: "zig",
        file_extensions: &["zig"],
        binary_names: &["zls"],
        install_hint: "install zls",
    },
    LspEntry {
        language_id: "elixir",
        file_extensions: &["ex", "exs"],
        binary_names: &["elixir-ls"],
        install_hint: "install elixir-ls",
    },
    LspEntry {
        language_id: "haskell",
        file_extensions: &["hs", "lhs"],
        binary_names: &["haskell-language-server-wrapper", "haskell-language-server"],
        install_hint: "ghcup install hls",
    },
    LspEntry {
        language_id: "ocaml",
        file_extensions: &["ml", "mli"],
        binary_names: &["ocamllsp"],
        install_hint: "opam install ocaml-lsp-server",
    },
    LspEntry {
        language_id: "svelte",
        file_extensions: &["svelte"],
        binary_names: &["svelte-language-server", "svelteserver"],
        install_hint: "npm i -g svelte-language-server",
    },
    LspEntry {
        language_id: "vue",
        file_extensions: &["vue"],
        binary_names: &["vue-language-server"],
        install_hint: "npm i -g @vue/language-server",
    },
];

/// Returns `true` if the given binary name can be found on the system PATH.
pub fn find_in_path(binary: &str) -> bool {
    #[cfg(unix)]
    let lookup_cmd = "which";
    #[cfg(windows)]
    let lookup_cmd = "where";

    Command::new(lookup_cmd)
        .arg(binary)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Attempts to detect an installed LSP server for the given `language_id`.
///
/// Returns `LspDetectionResult::Found` with the binary name if one of the
/// known server binaries is available on PATH, or
/// `LspDetectionResult::NotInstalled` with an install hint otherwise.
/// Returns `None` if the language is not in the registry.
pub fn detect_lsp_for_language(language_id: &str) -> Option<LspDetectionResult> {
    let entry = LSP_REGISTRY
        .iter()
        .find(|e| e.language_id == language_id)?;

    for &binary in entry.binary_names {
        if find_in_path(binary) {
            return Some(LspDetectionResult::Found {
                binary: binary.to_string(),
                language_id: entry.language_id.to_string(),
            });
        }
    }

    Some(LspDetectionResult::NotInstalled {
        language_id: entry.language_id.to_string(),
        install_hint: entry.install_hint.to_string(),
    })
}

/// Attempts to detect an installed LSP server for a file with the given
/// extension (without the leading dot).
///
/// Looks up the extension in the registry to find the corresponding language,
/// then delegates to [`detect_lsp_for_language`].
pub fn detect_lsp_for_extension(ext: &str) -> Option<LspDetectionResult> {
    let entry = LSP_REGISTRY
        .iter()
        .find(|e| e.file_extensions.contains(&ext))?;

    detect_lsp_for_language(entry.language_id)
}

/// Walks a workspace directory (up to `max_depth` levels deep) and returns a
/// deduplicated list of language IDs for which source files were found.
pub fn detect_workspace_languages(workspace: &Path) -> Vec<String> {
    let mut languages = HashSet::new();
    walk_dir(workspace, 0, 3, &mut languages);
    languages.into_iter().collect()
}

fn walk_dir(
    dir: &Path,
    current_depth: usize,
    max_depth: usize,
    languages: &mut HashSet<String>,
) {
    if current_depth > max_depth {
        return;
    }

    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();

        // Skip hidden directories and common non-source directories.
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
        }

        if path.is_dir() {
            walk_dir(&path, current_depth + 1, max_depth, languages);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            for entry in LSP_REGISTRY {
                if entry.file_extensions.contains(&ext) {
                    languages.insert(entry.language_id.to_string());
                    break;
                }
            }
        }
    }
}
