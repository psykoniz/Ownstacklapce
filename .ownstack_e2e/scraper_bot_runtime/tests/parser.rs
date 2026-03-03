use std::fs;
use scraper_bot_runtime::parse_html;

#[test]
fn test_parse_html() {
    let html = fs::read_to_string("fixtures/sample.html")
        .expect("Failed to read fixture file");
    
    let (title, links) = parse_html(&html);
    
    assert_eq!(title, "OwnStack Fixture");
    assert_eq!(links, vec!["/docs", "/contact"]);
}