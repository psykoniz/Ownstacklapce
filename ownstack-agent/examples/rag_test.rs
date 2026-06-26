//! Validates the SemanticIndex hashed-embedding fallback (no BERT model needed).
use ownstack_agent::index::SemanticIndex;
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let ws = PathBuf::from(std::env::var("TEST_WS").expect("TEST_WS"));
    std::fs::create_dir_all(&ws).ok();
    std::fs::write(ws.join("auth.rs"),
        "pub fn authenticate(user: &str, password: &str) -> bool {\n    // validate credentials and login the user session\n    !user.is_empty() && !password.is_empty()\n}\n").ok();
    std::fs::write(ws.join("mathx.rs"),
        "pub fn add(a: i32, b: i32) -> i32 { a + b }\npub fn multiply(a: i32, b: i32) -> i32 { a * b }\n").ok();
    std::fs::write(ws.join("db.rs"),
        "pub fn connect_database(url: &str) {}\npub fn run_sql_query(sql: &str) -> Vec<String> { vec![] }\n").ok();

    let mut idx = SemanticIndex::new(ws.clone());
    println!("init: {:?}", idx.init().await);
    println!("index_workspace: {:?}", idx.index_workspace().await);

    for q in ["authenticate user login credentials", "add multiply numbers", "database sql query"] {
        println!("\nquery: {:?}", q);
        match idx.search(q, 2).await {
            Ok(chunks) => {
                if chunks.is_empty() { println!("  (no results)"); }
                for c in &chunks {
                    println!("  -> {} L{}-{}: {}", c.path, c.start_line, c.end_line, c.content.lines().next().unwrap_or("").trim());
                }
            }
            Err(e) => println!("  ERR {}", e),
        }
    }
}
