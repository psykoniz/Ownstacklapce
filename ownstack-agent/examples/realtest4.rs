//! Fourth-wave functional tests: Git, Multivers, LSP.
use ownstack_agent::provider::LlmProvider;
use ownstack_agent::providers::openai_compatible::OpenAiCompatibleProvider;
use ownstack_agent::toolkits::multivers::VariantConfig;
use ownstack_agent::toolkits::{GitToolkit, LspToolkit, MultiversToolkit, Toolkit};
use ownstack_engine::{PolicyEngine, ProcessSandbox, Sandbox};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let provider: Arc<dyn LlmProvider + Send + Sync> =
        Arc::new(OpenAiCompatibleProvider::from_env().expect("provider from_env"));
    let base = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));

    // ── W4.1 git_status + W4.2 suggest_commit_message ──────────────────────
    {
        let ws = base.join("git");
        std::fs::create_dir_all(&ws).ok();
        let g = |args: &[&str]| {
            Command::new("git").args(args).current_dir(&ws).output().ok();
        };
        g(&["init", "-q"]);
        g(&["config", "user.email", "t@t.co"]);
        g(&["config", "user.name", "t"]);
        std::fs::write(ws.join("app.py"), "def add(a,b):\n    return a+b\n").ok();
        g(&["add", "."]);
        g(&["commit", "-qm", "init"]);
        std::fs::write(ws.join("app.py"), "def add(a,b):\n    return a+b\n\ndef sub(a,b):\n    return a-b\n").ok();
        g(&["add", "."]); // stage so the diff is visible to suggest_commit_message

        let policy: Arc<PolicyEngine> = Arc::new(PolicyEngine);
        let sandbox: Arc<dyn Sandbox + Send + Sync> = Arc::new(ProcessSandbox);
        let git = GitToolkit::new(ws.clone(), "git".to_string(), None, policy, sandbox, provider.clone());

        let st = git.execute("git_status", serde_json::json!({})).await;
        println!("\n######## W4.1 git_status ########");
        match st {
            Ok(x) => println!("success={} out={:?}", x.success, x.stdout.chars().take(200).collect::<String>().replace('\n', " ")),
            Err(e) => println!("ERR {:?}", e),
        }

        let t = Instant::now();
        let msg = git.suggest_commit_message().await;
        println!("\n######## W4.2 suggest_commit_message ({:.1}s) ########", t.elapsed().as_secs_f64());
        match msg {
            Ok(x) => println!("{}", x.stdout.chars().take(280).collect::<String>()),
            Err(e) => println!("ERR {:?}", e),
        }
    }

    // ── W4.3 Multivers fork_and_run (A/B) ──────────────────────────────────
    {
        let ws = base.join("mv");
        std::fs::create_dir_all(&ws).ok();
        let mv = MultiversToolkit::new(ws.clone());
        let mut variants = HashMap::new();
        variants.insert("A".to_string(), VariantConfig { env_vars: HashMap::new(), setup_commands: vec![] });
        let mut env_b = HashMap::new();
        env_b.insert("FOO".to_string(), "bar".to_string());
        variants.insert("B".to_string(), VariantConfig { env_vars: env_b, setup_commands: vec![] });
        let t = Instant::now();
        let run = mv.fork_and_run("echo variant-run", &variants).await;
        println!("\n######## W4.3 Multivers.fork_and_run ({:.1}s) ########", t.elapsed().as_secs_f64());
        println!("completed={} winner={:?} variants_run={}", run.completed, run.winner, run.results.len());
        for (name, r) in run.results.iter() {
            println!("  variant {} -> exit={} score={}", name, r.exit_code, r.score);
        }
    }

    // ── W4.4 LSP via rust-analyzer ─────────────────────────────────────────
    {
        let ws = base.join("lsp");
        std::fs::create_dir_all(ws.join("src")).ok();
        std::fs::write(ws.join("Cargo.toml"), "[package]\nname=\"t\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").ok();
        std::fs::write(ws.join("src").join("main.rs"), "fn main() { let x: i32 = \"bad\"; let _ = x; }\n").ok();
        let lsp = LspToolkit::new(ws.clone());
        let t = Instant::now();
        let conn = lsp.execute("lsp_auto_connect", serde_json::json!({})).await;
        println!("\n######## W4.4 LSP auto_connect ({:.1}s) ########", t.elapsed().as_secs_f64());
        match conn {
            Ok(x) => println!("success={} out={:?}", x.success, x.stdout.chars().take(160).collect::<String>().replace('\n', " ")),
            Err(e) => println!("ERR {:?}", e),
        }
        let diag = lsp.execute("lsp_diagnostics", serde_json::json!({"path":"src/main.rs"})).await;
        match diag {
            Ok(x) => println!("diagnostics success={} out={:?}", x.success, x.stdout.chars().take(260).collect::<String>().replace('\n', " ")),
            Err(e) => println!("diag ERR {:?}", e),
        }
    }
    println!("\n@@@@ WAVE-4 DONE @@@@");
}
