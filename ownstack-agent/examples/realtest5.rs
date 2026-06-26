//! Fifth-wave: InfraSense (system metrics) and ArtifactManager.
use ownstack_agent::artifact_manager::Artifact;
use ownstack_agent::{ArtifactManager, InfraSense};
use std::path::PathBuf;

fn main() {
    let base = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));

    // ── W5.1 InfraSense — real host metrics (drives the chat "Disk critical") ─
    {
        let ws = base.join("infra");
        std::fs::create_dir_all(&ws).ok();
        let m = InfraSense::collect_metrics(&ws);
        println!("\n######## W5.1 InfraSense ########");
        println!("summary: {}", m.summary());
        println!("memory_critical={} disk_critical={}", m.is_memory_critical(), m.is_disk_critical());
        let health = InfraSense::health_check(&ws);
        println!("health_check ({} items):", health.len());
        for h in health.iter().take(6) {
            println!("  - {}", h);
        }
    }

    // ── W5.2 ArtifactManager — persist artifacts ─────────────────────────────
    {
        let ws = base.join("artifacts");
        std::fs::create_dir_all(&ws).ok();
        let am = ArtifactManager::new(ws.clone());
        let arts = vec![
            Artifact { artifact_type: "code".to_string(), name: "hello.py".to_string(), content: "print('hi')\n".to_string() },
            Artifact { artifact_type: "doc".to_string(), name: "README.md".to_string(), content: "# Title\n".to_string() },
        ];
        let saved = am.save_artifacts(&arts);
        println!("\n######## W5.2 ArtifactManager ########");
        println!("saved {} artifact(s):", saved.len());
        for s in &saved {
            println!("  {}", s);
        }
    }
    println!("\n@@@@ WAVE-5 DONE @@@@");
}
