use std::net::IpAddr;

pub fn parse_insecure_ssl_suffixes(contents: &str) -> Result<Vec<String>, String> {
    let mut suffixes = Vec::new();

    for (index, raw_line) in contents.lines().enumerate() {
        let line_no = index + 1;
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.contains('*') {
            return Err(format!(
                "invalid insecure SSL suffix on line {line_no}: '*' wildcards are not supported"
            ));
        }

        let suffix = line.trim_end_matches('.').to_ascii_lowercase();
        if suffix.is_empty() {
            return Err(format!(
                "invalid insecure SSL suffix on line {line_no}: suffix cannot be empty"
            ));
        }

        if suffix.parse::<IpAddr>().is_ok() {
            return Err(format!(
                "invalid insecure SSL suffix on line {line_no}: IP addresses are not allowed"
            ));
        }

        if suffix.starts_with('.') || suffix.ends_with('.') || suffix.contains("..") {
            return Err(format!(
                "invalid insecure SSL suffix on line {line_no}: malformed host suffix '{line}'"
            ));
        }

        if !suffix
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.')
        {
            return Err(format!(
                "invalid insecure SSL suffix on line {line_no}: malformed host suffix '{line}'"
            ));
        }

        suffixes.push(suffix);
    }

    Ok(suffixes)
}
