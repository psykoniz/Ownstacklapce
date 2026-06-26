//! Third-wave functional tests: Healer, ProjectMemory, RAG, MCP, Vision.
use ownstack_agent::project_memory::ProjectMemory;
use ownstack_agent::provider::LlmProvider;
use ownstack_agent::providers::openai_compatible::OpenAiCompatibleProvider;
use ownstack_agent::toolkits::healer::{FailureAnalyzer, HealerToolkit};
use ownstack_agent::toolkits::mcp::{McpClient, McpServerConfig};
use ownstack_agent::toolkits::{Toolkit, VisionToolkit};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let provider: Arc<dyn LlmProvider + Send + Sync> =
        Arc::new(OpenAiCompatibleProvider::from_env().expect("provider from_env"));
    let base = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));

    // ── W3.1 Healer failure analysis (static, no LLM) ──────────────────────
    {
        let out = "error[E0425]: cannot find value `foo` in this scope\n  --> src/main.rs:3:5\nerror: aborting due to previous error";
        let failures = FailureAnalyzer::analyze(out, 101);
        println!("\n######## W3.1 FailureAnalyzer.analyze ########");
        println!("failures_detected={}", failures.len());
        for f in failures.iter().take(3) {
            println!("  {:?}", f);
        }
    }

    // ── W3.2 Healer self-heal a failing command (LLM) ──────────────────────
    {
        let ws = base.join("heal");
        std::fs::create_dir_all(&ws).ok();
        let healer = HealerToolkit::new(ws.clone(), Some(provider.clone()));
        let t = Instant::now();
        let session = healer.heal("python missing_script_xyz.py", 1).await;
        println!("\n######## W3.2 Healer.heal ({:.1}s) ########", t.elapsed().as_secs_f64());
        println!("healed={} attempts={}", session.healed, session.attempts.len());
        for a in session.attempts.iter().take(2) {
            println!("  attempt: {:?}", a);
        }
    }

    // ── W3.3 ProjectMemory rules loading (sync) ────────────────────────────
    {
        let ws = base.join("mem");
        std::fs::create_dir_all(ws.join(".ownstack")).ok();
        std::fs::write(
            ws.join(".ownstack").join("rules.md"),
            "# Project Rules\n- Always use 4-space indentation.\n- Never commit secrets.\n- Prefer async/await over raw threads.\n",
        ).ok();
        let mem = ProjectMemory::new(ws.clone());
        let rules = mem.load_rules();
        println!("\n######## W3.3 ProjectMemory.load_rules ########");
        match rules {
            Some(r) => println!("loaded {} chars; head:\n{}", r.len(), r.chars().take(160).collect::<String>()),
            None => println!("no rules loaded"),
        }
    }

    // ── W3.4 RAG semantic index init (local BERT) ──────────────────────────
    {
        let ws = base.join("rag");
        std::fs::create_dir_all(&ws).ok();
        std::fs::write(ws.join("doc.rs"), "pub fn authenticate(user: &str) {}\n").ok();
        let mut idx = ownstack_agent::index::SemanticIndex::new(ws.clone());
        let t = Instant::now();
        let r = idx.init().await;
        println!("\n######## W3.4 RAG SemanticIndex.init ({:.2}s) ########", t.elapsed().as_secs_f64());
        println!("init_result={:?}", r);
    }

    // ── W3.5 MCP connect to mock server + call tool ────────────────────────
    {
        let mut mcp = McpClient::new();
        let mock = std::env::var("MOCK_MCP").expect("MOCK_MCP");
        let cfg = McpServerConfig {
            name: "mock".to_string(),
            command: "python".to_string(),
            args: vec![mock],
            env: HashMap::new(),
        };
        let t = Instant::now();
        println!("\n######## W3.5 MCP ({}) ########", "mock server");
        match mcp.connect(cfg).await {
            Ok(()) => {
                println!("connect OK ({:.1}s)", t.elapsed().as_secs_f64());
                let r = mcp.call_tool("mock", "echo", serde_json::json!({"text":"hello-mcp"})).await;
                match r {
                    Ok(x) => println!("echo -> success={} stdout={:?}", x.success, x.stdout.chars().take(120).collect::<String>()),
                    Err(e) => println!("call_tool ERR {:?}", e),
                }
            }
            Err(e) => println!("connect ERR {:?}", e),
        }
    }

    // ── W3.6 Vision analyze_image (best effort, multi-modal) ───────────────
    {
        if let Ok(src) = std::env::var("VIS_IMG") {
            let ws = base.join("vis");
            std::fs::create_dir_all(&ws).ok();
            std::fs::copy(&src, ws.join("shot.png")).ok();
            let tk = VisionToolkit::new(ws.clone(), "vis".to_string())
                .with_provider(provider.clone());
            let t = Instant::now();
            let r = tk.execute("analyze_image", serde_json::json!({
                "image_path":"shot.png",
                "prompt":"Describe this screenshot in one short sentence."
            })).await;
            println!("\n######## W3.6 Vision.analyze_image ({:.1}s) ########", t.elapsed().as_secs_f64());
            match r {
                Ok(x) => println!("success={} stdout={:?}", x.success, x.stdout.chars().take(300).collect::<String>().replace('\n', " ")),
                Err(e) => println!("ERR {:?}", e),
            }
        } else {
            println!("\n######## W3.6 Vision ######## (skipped: VIS_IMG not set)");
        }
    }
    println!("\n@@@@ WAVE-3 DONE @@@@");
}
