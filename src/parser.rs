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

    let masked = strip_code_spans(&content);

    let re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    let mut seen = HashSet::new();
    let mut links = Vec::new();

    for cap in re.captures_iter(&masked) {
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

// Mask fenced (```...```) and inline (`...`) code spans with spaces of equal
// length, so [[wiki-links]] inside code are not extracted as real links.
fn strip_code_spans(content: &str) -> String {
    let fence_re = Regex::new(r"```[\s\S]*?```").unwrap();
    let inline_re = Regex::new(r"`[^`\n]*`").unwrap();

    let step1 = fence_re
        .replace_all(content, |c: &regex::Captures| " ".repeat(c[0].len()))
        .into_owned();

    inline_re
        .replace_all(&step1, |c: &regex::Captures| " ".repeat(c[0].len()))
        .into_owned()
}
