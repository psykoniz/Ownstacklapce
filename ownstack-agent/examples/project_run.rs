//! Validates the ProjectRunner state machine end-to-end against the live provider.
use ownstack_agent::orchestrator::{AgentOrchestrator, AgentRunMode};
use ownstack_agent::project_runner::{ProjectConfig, ProjectRunner};
use ownstack_agent::providers::openai_compatible::OpenAiCompatibleProvider;
use ownstack_agent::toolkits::CoreToolkit;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() {
    let provider = Arc::new(OpenAiCompatibleProvider::from_env().expect("provider from_env"));
    let ws = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));
    std::fs::create_dir_all(&ws).ok();

    let mut orch = AgentOrchestrator::new(provider.clone(), ws.clone(), 200_000, "project");
    orch.register_toolkit(Arc::new(CoreToolkit::new(ws.clone(), "project".to_string(), None)));
    orch.set_mode(AgentRunMode::Auto);

    let cfg = ProjectConfig {
        max_repair: 3,
        max_review: 2,
        test_command: Some("python test_calc.py".to_string()),
        max_units: 5,
    };
    let mut runner = ProjectRunner::new(orch, provider.clone(), ws.clone(), cfg);

    let goal = "Build a tiny Python calculator: a module calc.py exposing add(a,b) and sub(a,b), \
                and a test_calc.py that imports from calc and uses assert to verify add(2,3)==5 and sub(5,2)==3. \
                Running `python test_calc.py` must exit 0 with no output on success.";

    let t = Instant::now();
    let outcome = runner.run(goal).await;
    println!("\n=== ProjectOutcome ({:.1}s) ===", t.elapsed().as_secs_f64());
    println!("mission_id={} success={}", outcome.mission_id, outcome.success);
    for (i, u) in outcome.units.iter().enumerate() {
        println!("  unit {}: tests_passed={:?} repair={} review={} approved={}  | {}",
            i + 1, u.tests_passed, u.repair_attempts, u.review_cycles, u.approved,
            u.description.chars().take(60).collect::<String>());
    }

    // Persistence checks
    let wu = ws.join(".ownstack").join("work-units.json");
    println!("\nwork-units.json: {}", if wu.exists() { "present" } else { "MISSING" });
    if let Ok(s) = std::fs::read_to_string(&wu) { println!("{}", s.chars().take(400).collect::<String>()); }
    let missions = ws.join(".ownstack").join("missions");
    let mcount = std::fs::read_dir(&missions).map(|d| d.count()).unwrap_or(0);
    println!("persisted mission records: {}", mcount);

    let lessons = ws.join(".ownstack").join("lessons.md");
    println!("\n=== LEARN: lessons.md ===");
    println!("{}", std::fs::read_to_string(&lessons).unwrap_or_else(|_| "(none)".into()));

    println!("\n--- workspace files ---");
    for e in std::fs::read_dir(&ws).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_file() {
            let c = std::fs::read_to_string(&p).unwrap_or_default();
            println!("  {} ({} bytes)", p.file_name().unwrap().to_string_lossy(), c.len());
        }
    }
}
