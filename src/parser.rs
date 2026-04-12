use regex::Regex;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub struct Link {
    pub source: String,
    pub target: String,
}

pub fn parse_links(source_path: &str, abs_path: &Path) -> Vec<Link> {
    let content = match fs::read_to_string(abs_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: could not read {}: {}", abs_path.display(), e);
            return Vec::new();
        }
    };

    let re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for cap in re.captures_iter(&content) {
        let target = cap[1].trim().to_string();
        if seen.insert(target.clone()) {
            links.push(Link {
                source: source_path.to_string(),
                target,
            });
        }
    }

    links
}
