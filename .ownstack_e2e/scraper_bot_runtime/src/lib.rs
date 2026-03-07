pub fn parse_html(input: &str) -> (String, Vec<String>) {
    // Extract title
    let title = if let Some(start) = input.find("<title>") {
        if let Some(end) = input.find("</title>") {
            input[start + 7..end].to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Extract href links
    let mut links = Vec::new();
    let mut remaining = input;
    
    while let Some(start) = remaining.find("href=\"") {
        let start_idx = start + 6;
        remaining = &remaining[start_idx..];
        
        if let Some(end) = remaining.find('"') {
            let link = remaining[..end].to_string();
            if !link.is_empty() {
                links.push(link);
            }
            remaining = &remaining[end + 1..];
        } else {
            break;
        }
    }
    
    (title, links)
}