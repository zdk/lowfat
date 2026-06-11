//! Markdown compression — strip noise, keep structure.
//!
//! lite: strip badge images, HTML comments, normalize blanks
//! full: + collapse long code blocks, truncate tables
//! ultra: + collapse sections to headings + first paragraph

use std::sync::LazyLock;

use regex::Regex;

use crate::Level;

static BADGE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\[!\[.*?\]\(.*?\)\]\(.*?\)\s*$").unwrap());
static HTML_COMMENT_START: &str = "<!--";
static HTML_COMMENT_END: &str = "-->";

pub fn compress(content: &str, level: Level) -> String {
    match level {
        Level::Lite => lite(content),
        Level::Full => full(content),
        Level::Ultra => ultra(content),
    }
}

fn lite(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_html_comment = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Strip HTML comments
        if trimmed.contains(HTML_COMMENT_START) && !trimmed.contains(HTML_COMMENT_END) {
            in_html_comment = true;
            continue;
        }
        if in_html_comment {
            if trimmed.contains(HTML_COMMENT_END) {
                in_html_comment = false;
            }
            continue;
        }
        if trimmed.contains(HTML_COMMENT_START) && trimmed.contains(HTML_COMMENT_END) {
            continue;
        }

        // Strip badge images
        if BADGE_RE.is_match(trimmed) {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    normalize_blanks(&result)
}

fn full(content: &str) -> String {
    let base = lite(content);
    let mut result = String::with_capacity(base.len());
    let mut in_code_block = false;
    let mut code_block_lines = 0;
    let mut table_rows = 0;

    for line in base.lines() {
        let trimmed = line.trim();

        // Track fenced code blocks
        if trimmed.starts_with("```") {
            if in_code_block {
                // Closing fence
                if code_block_lines > 20 {
                    result.push_str(&format!("    ... [{} lines]\n", code_block_lines));
                }
                result.push_str(line);
                result.push('\n');
                in_code_block = false;
                code_block_lines = 0;
            } else {
                in_code_block = true;
                code_block_lines = 0;
                result.push_str(line);
                result.push('\n');
            }
            continue;
        }

        if in_code_block {
            code_block_lines += 1;
            if code_block_lines <= 20 {
                result.push_str(line);
                result.push('\n');
            }
            continue;
        }

        // Truncate tables >10 rows
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            table_rows += 1;
            if table_rows <= 10 {
                result.push_str(line);
                result.push('\n');
            } else if table_rows == 11 {
                result.push_str(&format!("| ... [more rows] |\n"));
            }
            continue;
        } else {
            table_rows = 0;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

fn ultra(content: &str) -> String {
    let base = full(content);
    let mut result = String::with_capacity(base.len() / 2);
    let mut after_heading = false;
    let mut paragraph_kept = false;

    for line in base.lines() {
        let trimmed = line.trim();

        // Always keep headings
        if trimmed.starts_with('#') {
            result.push_str(line);
            result.push('\n');
            after_heading = true;
            paragraph_kept = false;
            continue;
        }

        // Keep first non-empty paragraph after heading
        if after_heading {
            if trimmed.is_empty() {
                if paragraph_kept {
                    after_heading = false;
                }
                result.push('\n');
                continue;
            }
            paragraph_kept = true;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Skip other content at ultra level
        if trimmed.is_empty() {
            result.push('\n');
        }
    }

    normalize_blanks(&result)
}

fn normalize_blanks(content: &str) -> String {
    static MULTI_BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());
    MULTI_BLANK.replace_all(content, "\n\n").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_badges() {
        let input = "# Title\n\n[![CI](https://img.shields.io/badge)](https://ci.example)\n\nContent here.\n";
        let result = compress(input, Level::Lite);
        assert!(!result.contains("img.shields.io"));
        assert!(result.contains("# Title"));
        assert!(result.contains("Content here"));
    }

    #[test]
    fn strips_html_comments() {
        let input = "# Title\n<!-- TODO: fix this -->\nReal content.\n";
        let result = compress(input, Level::Lite);
        assert!(!result.contains("TODO"));
        assert!(result.contains("Real content"));
    }

    #[test]
    fn truncates_code_blocks() {
        let mut input = String::from("# Code\n\n```rust\n");
        for i in 0..50 {
            input.push_str(&format!("let x{} = {};\n", i, i));
        }
        input.push_str("```\n");

        let result = compress(&input, Level::Full);
        assert!(result.contains("```rust"));
        assert!(result.contains("... ["));
        assert!(result.contains("lines]"));
    }

    #[test]
    fn ultra_keeps_headings_and_first_para() {
        let input = "# Overview\n\nFirst paragraph here.\n\nSecond paragraph here.\n\n# Next\n\nAnother first.\n\nAnother second.\n";
        let result = compress(input, Level::Ultra);
        assert!(result.contains("# Overview"));
        assert!(result.contains("First paragraph"));
        assert!(!result.contains("Second paragraph"));
        assert!(result.contains("# Next"));
        assert!(result.contains("Another first"));
    }
}
