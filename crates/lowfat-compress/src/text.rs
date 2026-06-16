//! Fallback text compression for unknown file types.
//!
//! lite: normalize blanks
//! full: head + tail with line count
//! ultra: aggressive head with summary

use std::sync::LazyLock;

use regex::Regex;

use crate::Level;

static MULTI_BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

pub fn compress(content: &str, level: Level) -> String {
    match level {
        Level::Lite => normalize_blanks(content),
        Level::Full => head_tail(content, 200, 20),
        Level::Ultra => head_tail(content, 100, 10),
    }
}

fn normalize_blanks(content: &str) -> String {
    MULTI_BLANK.replace_all(content, "\n\n").to_string()
}

fn head_tail(content: &str, head: usize, tail: usize) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.len() <= head + tail {
        return normalize_blanks(content);
    }

    let mut result = String::with_capacity(content.len() / 2);
    for line in &lines[..head] {
        result.push_str(line);
        result.push('\n');
    }

    let omitted = lines.len() - head - tail;
    result.push_str(&format!("\n[... {} lines omitted ...]\n\n", omitted));

    for line in &lines[lines.len() - tail..] {
        result.push_str(line);
        result.push('\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_file_passthrough() {
        let input = "line1\nline2\nline3\n";
        assert_eq!(compress(input, Level::Full), input);
    }

    #[test]
    fn long_file_truncated() {
        let input: String = (0..500).map(|i| format!("line {}\n", i)).collect();
        let result = compress(&input, Level::Full);
        assert!(result.contains("lines omitted"));
        assert!(result.contains("line 0"));
        assert!(result.contains("line 499"));
        assert!(result.len() < input.len());
    }
}
