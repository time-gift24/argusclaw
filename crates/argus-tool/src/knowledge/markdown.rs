#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSection {
    pub path: String,
    pub title: String,
    pub anchor: String,
    pub start_line: usize,
    pub end_line: usize,
}

pub fn parse_markdown_sections(path: &str, content: &str) -> Vec<ParsedSection> {
    let mut sections: Vec<ParsedSection> = Vec::new();
    let total_lines = content.lines().count().max(1);

    for (index, line) in content.lines().enumerate() {
        let line_number = index + 1;
        let Some(title) = parse_heading(line) else {
            continue;
        };

        if let Some(previous) = sections.last_mut() {
            previous.end_line = line_number.saturating_sub(1);
        }

        sections.push(ParsedSection {
            path: path.to_string(),
            title: title.to_string(),
            anchor: slugify(title),
            start_line: line_number,
            end_line: total_lines,
        });
    }

    sections
}

fn parse_heading(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }

    let remainder = trimmed[hashes..].trim_start();
    if remainder.is_empty() {
        return None;
    }

    Some(remainder)
}

fn slugify(value: &str) -> String {
    let mut slug = String::new();
    let mut previous_was_dash = false;

    for ch in value.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
            previous_was_dash = false;
            continue;
        }

        if !previous_was_dash && !slug.is_empty() {
            slug.push('-');
            previous_was_dash = true;
        }
    }

    slug.trim_end_matches('-').to_string()
}
