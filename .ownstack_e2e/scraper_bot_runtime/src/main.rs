fn main() {
    use std::fs;
    use scraper_bot_runtime::parse_html;
    
    let html = fs::read_to_string("fixtures/sample.html")
        .expect("Failed to read fixture file");
    
    let (title, links) = parse_html(&html);
    
    println!("Title: {}", title);
    println!("Link count: {}", links.len());
    println!("Links: {:?}", links);
}