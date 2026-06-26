//! Real-conditions functional test of the OwnStack agent stack.
//! Drives the orchestrator with a live provider against a sandbox workspace.
//! Run: OPENAI_* env set + TEST_WS=<sandbox> cargo run -p ownstack-agent --example realtest
use ownstack_agent::orchestrator::{AgentOrchestrator, AgentRunMode};
use ownstack_agent::provider::LlmProvider;
use ownstack_agent::providers::openai_compatible::OpenAiCompatibleProvider;
use ownstack_agent::toolkits::CoreToolkit;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

fn build(provider: Arc<dyn LlmProvider>, ws: &PathBuf, sid: &str) -> AgentOrchestrator {
    std::fs::create_dir_all(ws).ok();
    let mut o = AgentOrchestrator::new(provider, ws.clone(), 200_000, sid);
    let tk = Arc::new(CoreToolkit::new(ws.clone(), sid.to_string(), None));
    o.register_toolkit(tk);
    o
}

fn show(ws: &PathBuf) -> String {
    let mut out = String::new();
    if let Ok(rd) = std::fs::read_dir(ws) {
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let content = std::fs::read_to_string(&p).unwrap_or_default();
            let snip: String = content.chars().take(200).collect();
            out.push_str(&format!("    [{}] {:?}\n", name, snip));
        }
    }
    if out.is_empty() { out.push_str("    (no files)\n"); }
    out
}

#[tokio::main]
async fn main() {
    let provider: Arc<dyn LlmProvider> =
        Arc::new(OpenAiCompatibleProvider::from_env().expect("provider from_env failed"));
    let base = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS not set"));

    macro_rules! run {
        ($name:expr, $sid:expr, $mode:expr, $method:ident, $prompt:expr) => {{
            let ws = base.join($sid);
            let mut o = build(provider.clone(), &ws, $sid);
            o.set_mode($mode);
            let t = Instant::now();
            let r = o.$method($prompt).await;
            let secs = t.elapsed().as_secs_f64();
            println!("\n######## {} ({:.1}s, mode {:?}) ########", $name, secs, $mode);
            match &r {
                Ok(s) => println!("RESULT_OK:\n{}", s),
                Err(e) => println!("RESULT_ERR: {}", e),
            }
            println!("WORKSPACE FILES:\n{}", show(&ws));
        }};
    }

    // T1 — pure reasoning, no tools needed
    run!("T1 reasoning", "t1", AgentRunMode::Ask, process,
        "What is 17*23? Reply with ONLY the number, nothing else.");

    // T2 — file creation via write tool
    run!("T2 write-file", "t2", AgentRunMode::Auto, process,
        "Create a file named hello.txt in the current workspace whose exact content is: Hello OwnStack");

    // T3 — read + edit existing file
    {
        let ws = base.join("t3");
        std::fs::create_dir_all(&ws).ok();
        std::fs::write(ws.join("note.txt"), "line one\n").ok();
    }
    run!("T3 read+edit", "t3", AgentRunMode::Auto, process,
        "Read note.txt, then append a second line that says: line two");

    // T4 — exec a command
    run!("T4 exec", "t4", AgentRunMode::Auto, process,
        "Use a shell command to create a file called made_by_exec.txt containing the word DONE, then confirm it exists.");

    // T5 — autonomous mission: write code + run it
    run!("T5 mission code+run", "t5", AgentRunMode::Auto, execute_mission,
        "Create a Python script fib.py that prints the first 10 Fibonacci numbers (space separated on one line), run it with python, and report the exact output.");

    // T6 — code quality (no exec)
    run!("T6 codegen", "t6", AgentRunMode::Ask, process,
        "Write a single Rust function `fn reverse(s: &str) -> String` that reverses a string by Unicode scalar. Return only the function in a code block.");

    println!("\n@@@@ ALL TESTS DONE @@@@");
}
