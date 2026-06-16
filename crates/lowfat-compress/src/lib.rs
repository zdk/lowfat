//! Content-aware file compression for LLM token reduction.
//!
//! Routes files by type (code, markdown, HTML, data, lock) and applies
//! level-appropriate compression. No dependency on lowfat internals.

mod code;
mod data;
mod detect;
mod html;
mod lock;
mod markdown;
mod text;

pub use detect::ContentType;

/// Compression intensity — mirrors lowfat-core's Level without depending on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Lite,
    Full,
    Ultra,
}

/// Compress file content based on file path and intensity level.
///
/// Returns compressed content. If compression is not worthwhile (< 10% reduction),
/// returns the original content unchanged.
pub fn compress(content: &str, file_path: &str, level: Level) -> String {
    if content.is_empty() {
        return String::new();
    }

    let content_type = detect::detect(file_path, content);
    let compressed = match content_type {
        ContentType::Code(lang) => code::compress(content, &lang, level),
        ContentType::Markdown => markdown::compress(content, level),
        ContentType::Html => html::compress(content, level),
        ContentType::Data => data::compress(content, file_path, level),
        ContentType::Lock => lock::compress(content, file_path),
        ContentType::Unknown => text::compress(content, level),
    };

    // Never hand back nothing for a file that had content: an all-comment file,
    // a refs-only .d.ts, or heading-less markdown at ultra can strip to empty —
    // show it verbatim instead of deleting it from context.
    if compressed.trim().is_empty() {
        return content.to_string();
    }

    // Only return compressed if we saved >10%
    if compressed.len() < content.len() * 9 / 10 {
        compressed
    } else {
        content.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input() {
        assert_eq!(compress("", "foo.rs", Level::Full), "");
    }

    #[test]
    fn passthrough_when_no_savings() {
        let content = "fn main() {}";
        assert_eq!(compress(content, "main.rs", Level::Full), content);
    }

    #[test]
    fn all_comment_file_not_emptied() {
        // A file that is 100% comments strips to empty — must pass through verbatim.
        let content = "# license line 1\n# license line 2\n# license line 3\n";
        assert_eq!(compress(content, "__init__.py", Level::Full), content);
    }
}
