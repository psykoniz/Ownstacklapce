//! Context enrichment for chat prompts.
//!
//! Builds a single, budget-bounded "context block" that is prepended to the
//! user's message before it reaches the LLM. Sources include retrieved code
//! chunks (RAG), explicit `@file` / `@symbol` mentions, current diagnostics,
//! and recent terminal output. Each section is formatted as Markdown with file
//! paths and line numbers so the model can cite locations precisely.

/// A single piece of context with a relevance score used for prioritisation.
#[derive(Debug, Clone)]
struct ContextItem {
    /// Section header, e.g. `src/main.rs (lines 10-40) — fn main`.
    heading: String,
    /// Raw body (code or text).
    body: String,
    /// Higher = more important; kept when the budget is tight.
    score: f32,
    /// Rough token estimate for budgeting.
    tokens: usize,
}

/// Accumulates context items and renders them within a token budget.
pub struct ContextBuilder {
    items: Vec<ContextItem>,
    /// Maximum tokens the rendered block may consume.
    budget_tokens: usize,
}

/// Rough token estimate: ~4 chars per token is the standard heuristic.
fn estimate_tokens(s: &str) -> usize {
    (s.len() / 4).max(1)
}

impl ContextBuilder {
    pub fn new(budget_tokens: usize) -> Self {
        Self {
            items: Vec::new(),
            budget_tokens,
        }
    }

    /// Add a retrieved code chunk (RAG). Score reflects search rank.
    pub fn add_code(
        &mut self,
        path: &str,
        start_line: usize,
        end_line: usize,
        symbol: Option<&str>,
        body: &str,
        score: f32,
    ) {
        let sym = symbol
            .map(|s| format!(" — {s}"))
            .unwrap_or_default();
        let heading = format!("{path} (lines {start_line}-{end_line}){sym}");
        let tokens = estimate_tokens(body) + estimate_tokens(&heading);
        self.items.push(ContextItem {
            heading,
            body: body.to_string(),
            score,
            tokens,
        });
    }

    /// Add an explicit `@file` mention — always high priority.
    pub fn add_file(&mut self, path: &str, body: &str) {
        let tokens = estimate_tokens(body) + estimate_tokens(path);
        self.items.push(ContextItem {
            heading: format!("{path} (full file, @mentioned)"),
            body: body.to_string(),
            score: 100.0,
            tokens,
        });
    }

    /// Add the current LSP diagnostics block.
    pub fn add_diagnostics(&mut self, body: &str) {
        if body.trim().is_empty() {
            return;
        }
        let tokens = estimate_tokens(body);
        self.items.push(ContextItem {
            heading: "Current diagnostics".to_string(),
            body: body.to_string(),
            score: 90.0,
            tokens,
        });
    }

    /// Add recent terminal output.
    pub fn add_terminal(&mut self, body: &str) {
        if body.trim().is_empty() {
            return;
        }
        let tokens = estimate_tokens(body);
        self.items.push(ContextItem {
            heading: "Recent terminal output".to_string(),
            body: body.to_string(),
            score: 80.0,
            tokens,
        });
    }

    /// True when no context has been gathered.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Render the context block as Markdown, dropping the lowest-scoring items
    /// once the token budget is exhausted. Returns `None` when empty.
    pub fn render(mut self) -> Option<String> {
        if self.items.is_empty() {
            return None;
        }

        // Highest score first; stable so equal scores keep insertion order.
        self.items
            .sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        let mut used = 0usize;
        let mut sections = Vec::new();
        for item in self.items {
            if used + item.tokens > self.budget_tokens && !sections.is_empty() {
                continue;
            }
            used += item.tokens;
            sections.push(format!("### {}\n```\n{}\n```", item.heading, item.body));
        }

        if sections.is_empty() {
            return None;
        }

        Some(format!(
            "## Relevant context (retrieved from your workspace)\n\n{}\n",
            sections.join("\n\n")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_builder_renders_none() {
        let b = ContextBuilder::new(1000);
        assert!(b.is_empty());
        assert!(b.render().is_none());
    }

    #[test]
    fn renders_code_section_with_location() {
        let mut b = ContextBuilder::new(1000);
        b.add_code("src/a.rs", 10, 20, Some("fn a"), "let x = 1;", 1.0);
        let out = b.render().unwrap();
        assert!(out.contains("src/a.rs (lines 10-20) — fn a"));
        assert!(out.contains("let x = 1;"));
        assert!(out.contains("Relevant context"));
    }

    #[test]
    fn file_mention_outranks_code() {
        let mut b = ContextBuilder::new(1000);
        b.add_code("low.rs", 1, 2, None, "low", 1.0);
        b.add_file("high.rs", "high");
        let out = b.render().unwrap();
        let high_pos = out.find("high.rs").unwrap();
        let low_pos = out.find("low.rs").unwrap();
        assert!(high_pos < low_pos, "file mention should render first");
    }

    #[test]
    fn budget_drops_low_priority_items() {
        // Tiny budget: only the highest-priority item fits.
        let mut b = ContextBuilder::new(20);
        b.add_code("a.rs", 1, 50, None, &"x".repeat(400), 1.0);
        b.add_file("b.rs", &"y".repeat(400));
        let out = b.render().unwrap();
        // b.rs (file mention, score 100) wins; a.rs is dropped.
        assert!(out.contains("b.rs"));
        assert!(!out.contains("a.rs"));
    }

    #[test]
    fn at_least_one_item_always_renders() {
        // Even when the single item exceeds budget, it is kept.
        let mut b = ContextBuilder::new(1);
        b.add_file("big.rs", &"z".repeat(10_000));
        assert!(b.render().is_some());
    }

    #[test]
    fn empty_diagnostics_and_terminal_are_ignored() {
        let mut b = ContextBuilder::new(1000);
        b.add_diagnostics("   ");
        b.add_terminal("");
        assert!(b.is_empty());
    }

    #[test]
    fn diagnostics_and_terminal_render() {
        let mut b = ContextBuilder::new(1000);
        b.add_diagnostics("error[E0382]: borrow of moved value");
        b.add_terminal("$ cargo build\nerror: ...");
        let out = b.render().unwrap();
        assert!(out.contains("Current diagnostics"));
        assert!(out.contains("Recent terminal output"));
    }

    #[test]
    fn token_estimate_is_positive() {
        assert!(estimate_tokens("") >= 1);
        assert!(estimate_tokens("abcd") >= 1);
    }
}
