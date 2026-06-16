//! Lock file compression — extreme summarization.
//!
//! Lock files are almost pure noise for LLMs. Replace with a summary
//! of dependency count + top-level deps.

use std::path::Path;

pub fn compress(content: &str, file_path: &str) -> String {
    let filename = Path::new(file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("lock");

    let line_count = content.lines().count();

    match filename {
        "Cargo.lock" => summarize_cargo_lock(content, line_count),
        "package-lock.json" => summarize_npm_lock(content, line_count),
        "yarn.lock" => summarize_yarn_lock(content, line_count),
        _ => summarize_generic(filename, content, line_count),
    }
}

fn summarize_cargo_lock(content: &str, line_count: usize) -> String {
    // Count [[package]] entries
    let pkg_count = content.matches("[[package]]").count();

    // Extract top-level package names (first few)
    let names: Vec<&str> = content
        .lines()
        .filter(|l| l.starts_with("name = "))
        .take(10)
        .map(|l| l.trim_start_matches("name = ").trim_matches('"'))
        .collect();

    format!(
        "# Cargo.lock: {} packages ({} lines)\n# Top packages: {}\n# [full lock file omitted — use `cargo tree` for dependency graph]\n",
        pkg_count,
        line_count,
        names.join(", ")
    )
}

fn summarize_npm_lock(content: &str, line_count: usize) -> String {
    // Count "node_modules/" entries (rough package count)
    let pkg_count = content.matches("node_modules/").count();
    format!(
        "// package-lock.json: ~{} packages ({} lines)\n// [full lock file omitted]\n",
        pkg_count, line_count
    )
}

fn summarize_yarn_lock(content: &str, line_count: usize) -> String {
    // Count top-level entries (lines that don't start with space)
    let entry_count = content
        .lines()
        .filter(|l| !l.starts_with(' ') && !l.is_empty() && !l.starts_with('#'))
        .count();
    format!(
        "# yarn.lock: ~{} entries ({} lines)\n# [full lock file omitted]\n",
        entry_count, line_count
    )
}

fn summarize_generic(filename: &str, _content: &str, line_count: usize) -> String {
    format!(
        "# {}: {} lines\n# [lock file omitted — not useful for LLM context]\n",
        filename, line_count
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_lock_summary() {
        let input = "[[package]]\nname = \"serde\"\nversion = \"1.0\"\n\n[[package]]\nname = \"tokio\"\nversion = \"1.0\"\n";
        let result = compress(input, "Cargo.lock");
        assert!(result.contains("2 packages"));
        assert!(result.contains("serde"));
        assert!(!result.contains("version"));
    }

    #[test]
    fn generic_lock() {
        let input = "a = 1\nb = 2\nc = 3\n";
        let result = compress(input, "Gemfile.lock");
        assert!(result.contains("3 lines"));
        assert!(result.contains("omitted"));
    }
}
