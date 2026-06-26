//! OwnStack agent evaluation suite.
//!
//! Consolidates the realtest cases into a named, scored eval run against the
//! live provider. Produces a JSON + Markdown report under `.ownstack/evals/`.
//!
//! Usage:
//!   OPENAI_API_KEY=... OPENAI_BASE_URL=... OPENAI_MODEL=... OPENAI_WIRE_API=chat \
//!   cargo run -p ownstack-agent --example evals
use ownstack_agent::orchestrator::{AgentOrchestrator, AgentRunMode};
use ownstack_agent::providers::openai_compatible::OpenAiCompatibleProvider;
use ownstack_agent::repomap::RepoMap;
use ownstack_agent::toolkits::{CoreToolkit, ExtraToolkit, Toolkit};
use serde_json::json;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

struct EvalResult {
    name: &'static str,
    category: &'static str,
    passed: bool,
    score: u32, // 0..=100
    secs: f64,
    detail: String,
}

fn build_orch(
    provider: &Arc<OpenAiCompatibleProvider>,
    ws: &PathBuf,
    sid: &str,
    mode: AgentRunMode,
) -> AgentOrchestrator {
    std::fs::create_dir_all(ws).ok();
    let mut o = AgentOrchestrator::new(provider.clone(), ws.clone(), 200_000, sid);
    o.register_toolkit(Arc::new(CoreToolkit::new(ws.clone(), sid.to_string(), None)));
    o.set_mode(mode);
    o
}

#[tokio::main]
async fn main() {
    let provider = Arc::new(
        OpenAiCompatibleProvider::from_env().expect("provider from_env failed"),
    );
    let base = PathBuf::from(
        std::env::var("TEST_WS")
            .unwrap_or_else(|_| std::env::temp_dir().join("ownstack-evals").to_string_lossy().into_owned()),
    );
    std::fs::create_dir_all(&base).ok();

    let mut results: Vec<EvalResult> = Vec::new();
    macro_rules! push {
        ($name:expr, $cat:expr, $passed:expr, $score:expr, $secs:expr, $detail:expr) => {
            let r = EvalResult { name: $name, category: $cat, passed: $passed, score: $score, secs: $secs, detail: $detail };
            println!("[{:>4}] {:<22} {:>5.1}s  {}", if r.passed {"PASS"} else {"FAIL"}, r.name, r.secs, r.detail.chars().take(70).collect::<String>());
            results.push(r);
        };
    }

    // E1 — reasoning
    {
        let mut o = build_orch(&provider, &base.join("e1"), "e1", AgentRunMode::Ask);
        let t = Instant::now();
        let r = o.process("What is 17*23? Reply with ONLY the number.").await.unwrap_or_default();
        let pass = r.contains("391");
        push!("reasoning", "core", pass, if pass {100} else {0}, t.elapsed().as_secs_f64(), r.chars().take(60).collect());
    }
    // E2 — code generation
    {
        let mut o = build_orch(&provider, &base.join("e2"), "e2", AgentRunMode::Ask);
        let t = Instant::now();
        let r = o.process("Write a Rust fn `reverse(s: &str) -> String` that reverses by char. Only the function.").await.unwrap_or_default();
        let pass = r.contains("rev()") || r.contains(".chars()");
        push!("codegen", "core", pass, if pass {100} else {0}, t.elapsed().as_secs_f64(), r.replace('\n'," ").chars().take(60).collect());
    }
    // E3 — file write (tool)
    {
        let ws = base.join("e3");
        let mut o = build_orch(&provider, &ws, "e3", AgentRunMode::Auto);
        let t = Instant::now();
        let _ = o.process("Create a file named hello.txt with exactly this content: Hello OwnStack").await;
        let got = std::fs::read_to_string(ws.join("hello.txt")).unwrap_or_default();
        let pass = got.contains("Hello OwnStack");
        push!("file_write", "tools", pass, if pass {100} else {0}, t.elapsed().as_secs_f64(), format!("file={:?}", got.trim()));
    }
    // E4 — read + edit (tool)
    {
        let ws = base.join("e4");
        std::fs::create_dir_all(&ws).ok();
        std::fs::write(ws.join("note.txt"), "line one\n").ok();
        let mut o = build_orch(&provider, &ws, "e4", AgentRunMode::Auto);
        let t = Instant::now();
        let _ = o.process("Read note.txt and append a second line that says: line two").await;
        let got = std::fs::read_to_string(ws.join("note.txt")).unwrap_or_default();
        let pass = got.contains("line one") && got.contains("line two");
        push!("file_edit", "tools", pass, if pass {100} else {0}, t.elapsed().as_secs_f64(), format!("file={:?}", got.replace('\n',"\\n")));
    }
    // E5 — exec redirect (validates the cmd /C fix end-to-end)
    {
        let ws = base.join("e5");
        let mut o = build_orch(&provider, &ws, "e5", AgentRunMode::Auto);
        let t = Instant::now();
        let _ = o.process("Use a single shell command with a redirect to create a file made.txt containing the word DONE.").await;
        let pass = std::fs::read_to_string(ws.join("made.txt")).map(|c| c.contains("DONE")).unwrap_or(false);
        push!("exec_redirect", "tools", pass, if pass {100} else {0}, t.elapsed().as_secs_f64(), if pass {"made.txt created".into()} else {"file missing".into()});
    }
    // E6 — RepoMap (no LLM)
    {
        let ws = base.join("e6");
        std::fs::create_dir_all(&ws).ok();
        std::fs::write(ws.join("a.rs"), "pub fn login() {}\npub fn logout() {}\nstruct User { id: u32 }\n").ok();
        std::fs::write(ws.join("b.py"), "def add(a,b):\n    return a+b\nclass Calc:\n    def mul(self,x,y):\n        return x*y\n").ok();
        let t = Instant::now();
        let n = RepoMap::new(ws.clone()).scan().len();
        let pass = n >= 4;
        push!("repomap", "context", pass, (n.min(6) as u32) * 100 / 6, t.elapsed().as_secs_f64(), format!("{} symbols", n));
    }
    // E7 — specialist: security
    {
        let tk = ExtraToolkit::new(Some(provider.clone()));
        let code = "def get_user(uid):\n    return db.execute(\"SELECT * FROM users WHERE id = \" + uid)";
        let t = Instant::now();
        let out = tk.execute("delegate_task", json!({"role":"security","instructions":format!("Audit for vulnerabilities:\n{}", code)})).await
            .map(|r| r.stdout).unwrap_or_default().to_lowercase();
        let pass = out.contains("injection");
        push!("specialist_security", "specialists", pass, if pass {100} else {0}, t.elapsed().as_secs_f64(), if pass {"SQLi flagged".into()} else {"missed".into()});
    }

    // ── Aggregate + report ─────────────────────────────────────────────────
    let total = results.len();
    let passed = results.iter().filter(|r| r.passed).count();
    let avg = if total > 0 { results.iter().map(|r| r.score).sum::<u32>() / total as u32 } else { 0 };
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();

    let report = json!({
        "timestamp": ts,
        "model": std::env::var("OPENAI_MODEL").unwrap_or_default(),
        "summary": { "total": total, "passed": passed, "avg_score": avg },
        "cases": results.iter().map(|r| json!({
            "name": r.name, "category": r.category, "passed": r.passed,
            "score": r.score, "secs": (r.secs*10.0).round()/10.0, "detail": r.detail
        })).collect::<Vec<_>>(),
    });

    let out_dir = std::env::current_dir().unwrap_or_default().join(".ownstack").join("evals");
    std::fs::create_dir_all(&out_dir).ok();
    let json_path = out_dir.join(format!("eval-{ts}.json"));
    std::fs::write(&json_path, serde_json::to_string_pretty(&report).unwrap_or_default()).ok();

    let mut md = format!("# OwnStack eval report\n\nModel: {} — {}/{} passed, avg score {}/100\n\n| Case | Cat | Pass | Score | s |\n|---|---|---|---|---|\n",
        std::env::var("OPENAI_MODEL").unwrap_or_default(), passed, total, avg);
    for r in &results {
        md.push_str(&format!("| {} | {} | {} | {} | {:.1} |\n", r.name, r.category, if r.passed {"✅"} else {"❌"}, r.score, r.secs));
    }
    let md_path = out_dir.join(format!("eval-{ts}.md"));
    std::fs::write(&md_path, &md).ok();

    println!("\n=== SUMMARY: {passed}/{total} passed, avg score {avg}/100 ===");
    println!("report: {}", json_path.display());
    println!("report: {}", md_path.display());
}
