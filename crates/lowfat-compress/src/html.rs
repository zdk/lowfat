//! HTML compression — extract meaningful text, strip noise.
//!
//! lite: strip <style>, <script>, HTML comments, inline styles
//! full: + strip class/id/data-* attributes
//! ultra: + text extraction only (keep structure tags)

use std::sync::LazyLock;

use regex::Regex;

use crate::Level;

static STYLE_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<style[^>]*>.*?</style>").unwrap());
static SCRIPT_BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<script[^>]*>.*?</script>").unwrap());
static HTML_COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").unwrap());
static INLINE_STYLE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"\s+style="[^"]*""#).unwrap());
static CLASS_ATTR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"\s+(class|id|data-[\w-]+)="[^"]*""#).unwrap());
// Only match real tags (start with letter/!/?), so prose like "a < b and c > d" survives.
static ALL_TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</?[a-zA-Z!?][^>]*>").unwrap());
static MULTI_SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]{2,}").unwrap());
static MULTI_BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

pub fn compress(content: &str, level: Level) -> String {
    match level {
        Level::Lite => lite(content),
        Level::Full => full(content),
        Level::Ultra => ultra(content),
    }
}

fn lite(content: &str) -> String {
    let s = STYLE_BLOCK.replace_all(content, "");
    let s = SCRIPT_BLOCK.replace_all(&s, "");
    let s = HTML_COMMENT.replace_all(&s, "");
    let s = INLINE_STYLE.replace_all(&s, "");
    MULTI_BLANK.replace_all(&s, "\n\n").to_string()
}

fn full(content: &str) -> String {
    let s = lite(content);
    let s = CLASS_ATTR.replace_all(&s, "");
    MULTI_BLANK.replace_all(&s, "\n\n").to_string()
}

fn ultra(content: &str) -> String {
    let s = full(content);
    // Strip all tags, keep text content
    let s = ALL_TAGS.replace_all(&s, " ");
    let s = MULTI_SPACE.replace_all(&s, " ");
    // Normalize: trim each line, collapse blanks
    let result: String = s
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    MULTI_BLANK.replace_all(&result, "\n\n").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_style_and_script() {
        let input = "<html><head><style>body { color: red; }</style></head><body><script>alert(1);</script><p>Hello</p></body></html>";
        let result = compress(input, Level::Lite);
        assert!(!result.contains("color: red"));
        assert!(!result.contains("alert"));
        assert!(result.contains("Hello"));
    }

    #[test]
    fn strips_class_attributes() {
        let input =
            r#"<div class="container mx-auto" id="main" data-testid="root"><p>Text</p></div>"#;
        let result = compress(input, Level::Full);
        assert!(!result.contains("container"));
        assert!(!result.contains("data-testid"));
        assert!(result.contains("Text"));
    }

    #[test]
    fn ultra_keeps_comparison_text() {
        // Regex must not eat "< 10 and a >" as if it were a tag.
        let input = "<p>5 < 10 and a > b</p>";
        let result = compress(input, Level::Ultra);
        assert!(result.contains("5 < 10 and a > b"), "got: {result}");
    }

    #[test]
    fn ultra_extracts_text() {
        let input = "<h1>Title</h1><p>Some <strong>bold</strong> text.</p><ul><li>Item 1</li><li>Item 2</li></ul>";
        let result = compress(input, Level::Ultra);
        assert!(result.contains("Title"));
        assert!(result.contains("bold"));
        assert!(result.contains("Item 1"));
        assert!(!result.contains("<h1>"));
    }
}
