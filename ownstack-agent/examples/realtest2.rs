//! Second-wave functional tests: RepoMap, specialist delegation, browser.
use ownstack_agent::provider::LlmProvider;
use ownstack_agent::providers::openai_compatible::OpenAiCompatibleProvider;
use ownstack_agent::repomap::RepoMap;
use ownstack_agent::toolkits::{BrowserToolkit, ExtraToolkit, Toolkit};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let provider: Arc<dyn LlmProvider + Send + Sync> =
        Arc::new(OpenAiCompatibleProvider::from_env().expect("provider from_env"));
    let base = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));

    // ── W1: RepoMap scan (no LLM) ──────────────────────────────────────────
    {
        let src = base.join("rm");
        std::fs::create_dir_all(&src).ok();
        std::fs::write(
            src.join("a.rs"),
            "pub fn login() {}\npub fn logout() {}\nstruct User { id: u32 }\nimpl User { fn display_name(&self) -> String { String::new() } }\n",
        ).ok();
        std::fs::write(
            src.join("b.py"),
            "def add(a, b):\n    return a + b\n\nclass Calc:\n    def mul(self, x, y):\n        return x * y\n",
        ).ok();
        let mut rm = RepoMap::new(src.clone());
        let t = Instant::now();
        let syms = rm.scan();
        println!("\n######## W1 RepoMap.scan ({:.2}s) ########", t.elapsed().as_secs_f64());
        println!("symbols_found={}", syms.len());
        for s in syms.iter().take(12) {
            println!("  {:?} {} :{}", s.kind, s.name, s.line);
        }
    }

    // ── W2: Security specialist on a SQL-injection snippet ─────────────────
    {
        let tk = ExtraToolkit::new(Some(provider.clone()));
        let code = "def get_user(uid):\n    q = \"SELECT * FROM users WHERE id = \" + uid\n    return db.execute(q)";
        let t = Instant::now();
        let r = tk.execute("delegate_task", serde_json::json!({
            "role":"security",
            "instructions": format!("Audit this Python code for vulnerabilities:\n{}", code)
        })).await;
        println!("\n######## W2 specialist=security ({:.1}s) ########", t.elapsed().as_secs_f64());
        match r { Ok(x) => println!("{}", x.stdout.chars().take(700).collect::<String>()), Err(e) => println!("ERR {:?}", e) }
    }

    // ── W3: Reviewer specialist on a div-by-zero bug ───────────────────────
    {
        let tk = ExtraToolkit::new(Some(provider.clone()));
        let code = "fn div(a: i32, b: i32) -> i32 { a / b }";
        let t = Instant::now();
        let r = tk.execute("delegate_task", serde_json::json!({
            "role":"reviewer",
            "instructions": format!("Review this Rust function. List bugs with severity:\n{}", code)
        })).await;
        println!("\n######## W3 specialist=reviewer ({:.1}s) ########", t.elapsed().as_secs_f64());
        match r { Ok(x) => println!("{}", x.stdout.chars().take(500).collect::<String>()), Err(e) => println!("ERR {:?}", e) }
    }

    // ── W4: Browser (best effort — needs headless Chrome) ──────────────────
    {
        let tk = BrowserToolkit;
        let t = Instant::now();
        let r = tk.execute("browse_url", serde_json::json!({"url":"https://example.com"})).await;
        println!("\n######## W4 browser browse_url ({:.1}s) ########", t.elapsed().as_secs_f64());
        match r {
            Ok(x) => println!("success={} stdout={:?}", x.success, x.stdout.chars().take(200).collect::<String>().replace('\n', " ")),
            Err(e) => println!("ERR {:?}", e),
        }
    }
    println!("\n@@@@ WAVE-2 DONE @@@@");
}
