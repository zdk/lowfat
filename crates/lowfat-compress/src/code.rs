//! Language-aware source code compression.
//!
//! Uses the data-driven LangSpec to strip comments and collapse function bodies.
//! lite: block comments + blank normalization
//! full: all comments (keep doc comments) + blank normalization
//! ultra: + collapse function bodies to signatures

use std::sync::LazyLock;

use regex::Regex;

use crate::detect::{LangId, LangSpec, ScopeStyle};
use crate::Level;

pub fn compress(content: &str, lang: &LangId, level: Level) -> String {
    match level {
        Level::Lite => {
            let s = strip_block_comments(content, lang.spec);
            normalize_blanks(&s)
        }
        Level::Full => {
            let s = strip_block_comments(content, lang.spec);
            let s = strip_line_comments(&s, lang.spec, true);
            normalize_blanks(&s)
        }
        Level::Ultra => {
            let s = strip_block_comments(content, lang.spec);
            let s = strip_line_comments(&s, lang.spec, true);
            let s = normalize_blanks(&s);
            collapse_bodies(&s, lang.spec)
        }
    }
}

// ── Comment stripping ───────────────────────────────────────────────

fn strip_block_comments(content: &str, spec: &LangSpec) -> String {
    let (start, end) = match spec.block_comment {
        Some(pair) => pair,
        None => return content.to_string(),
    };
    // When the doc marker IS the block opener (Python """), docstrings span multiple
    // lines — keep them verbatim and balanced so later passes aren't confused.
    let docstring = spec.doc_comment.filter(|d| *d == start);

    let mut result = String::with_capacity(content.len());
    let mut in_block = false;
    let mut in_string = false;
    let mut in_doc = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Keep a multi-line docstring intact until its closing marker.
        if in_doc {
            result.push_str(line);
            result.push('\n');
            if trimmed.contains(end) {
                in_doc = false;
            }
            continue;
        }
        if let Some(q) = docstring {
            if trimmed.starts_with(q) {
                result.push_str(line);
                result.push('\n');
                // Opener with no closer on the same line → swallow until it closes.
                if !trimmed[q.len()..].contains(q) {
                    in_doc = true;
                }
                continue;
            }
        }

        // Simple string detection: skip lines that are clearly inside a string literal
        if !in_block {
            let quote_count = line.matches('"').count();
            if quote_count % 2 != 0 {
                in_string = !in_string;
            }
            if in_string {
                result.push_str(line);
                result.push('\n');
                continue;
            }
        }

        if in_block {
            if trimmed.contains(end) {
                in_block = false;
            }
            continue;
        }

        // Keep doc block comments (e.g. /** ... */ or """...""" used as docstrings)
        if let Some(doc) = spec.doc_comment {
            if trimmed.starts_with(doc) {
                result.push_str(line);
                result.push('\n');
                continue;
            }
        }

        if trimmed.contains(start) && !trimmed.contains(end) {
            // Keep any code before the comment opener
            if let Some(pos) = line.find(start) {
                let before = line[..pos].trim_end();
                if !before.is_empty() {
                    result.push_str(before);
                    result.push('\n');
                }
            }
            in_block = true;
            continue;
        }
        // Single-line block comment (/* ... */ on one line) — strip only the comment
        if trimmed.contains(start) && trimmed.contains(end) {
            if let (Some(s), Some(e)) = (line.find(start), line.find(end)) {
                let before = &line[..s];
                let after = &line[e + end.len()..];
                let cleaned = format!("{}{}", before.trim_end(), after.trim_start());
                if !cleaned.trim().is_empty() {
                    result.push_str(&cleaned);
                    result.push('\n');
                }
            }
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

fn strip_line_comments(content: &str, spec: &LangSpec, keep_doc: bool) -> String {
    let prefix = match spec.line_comment {
        Some(p) => p,
        None => return content.to_string(),
    };

    let mut result = String::with_capacity(content.len());

    for line in content.lines() {
        let trimmed = line.trim();

        // Keep doc comments
        if keep_doc {
            if let Some(doc) = spec.doc_comment {
                if trimmed.starts_with(doc) {
                    result.push_str(line);
                    result.push('\n');
                    continue;
                }
            }
        }

        // Skip pure comment lines
        if trimmed.starts_with(prefix) {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

// ── Blank line normalization ────────────────────────────────────────

static MULTI_BLANK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

fn normalize_blanks(content: &str) -> String {
    MULTI_BLANK.replace_all(content, "\n\n").to_string()
}

// ── Body collapsing (ultra level) ───────────────────────────────────

fn collapse_bodies(content: &str, spec: &LangSpec) -> String {
    match spec.scope {
        ScopeStyle::Braces => collapse_braces(content, spec),
        ScopeStyle::Indentation => collapse_indent(content, spec),
        ScopeStyle::DoEnd => collapse_do_end(content, spec),
        ScopeStyle::None => content.to_string(),
    }
}

/// Collapse brace-delimited bodies (Rust, Go, JS, Java, C).
/// Keeps signature line + opening brace, replaces body with "...", keeps closing brace.
fn collapse_braces(content: &str, spec: &LangSpec) -> String {
    let sig_re = build_signature_regex(spec);
    let import_re = build_import_regex(spec);
    let mut result = String::with_capacity(content.len() / 2);
    let mut depth: i32 = 0;
    let mut in_body = false;

    for line in content.lines() {
        let trimmed = line.trim();

        // Always keep imports
        if import_re.is_match(trimmed) {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Detect signature — start tracking body
        if !in_body && sig_re.is_match(trimmed) {
            result.push_str(line);
            result.push('\n');
            if trimmed.ends_with('{') {
                in_body = true;
                depth = 1;
            }
            continue;
        }

        if !in_body {
            // Opening brace on next line after signature
            if trimmed == "{" && result.ends_with('\n') {
                let prev_line = result.trim_end().lines().last().unwrap_or("");
                if sig_re.is_match(prev_line.trim()) {
                    result.push_str(line);
                    result.push('\n');
                    in_body = true;
                    depth = 1;
                    continue;
                }
            }
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Inside body — count braces
        depth += trimmed.matches('{').count() as i32;
        depth -= trimmed.matches('}').count() as i32;

        if depth <= 0 {
            result.push_str("    ...\n");
            result.push_str(line);
            result.push('\n');
            in_body = false;
            depth = 0;
        }
    }

    result
}

/// Collapse indent-based bodies (Python).
/// Keeps the `def`/`class` line, replaces indented body with "...".
fn collapse_indent(content: &str, spec: &LangSpec) -> String {
    let sig_re = build_signature_regex(spec);
    let import_re = build_import_regex(spec);
    let mut result = String::with_capacity(content.len() / 2);
    let mut sig_indent: Option<usize> = None;
    let mut in_docstring: Option<&'static str> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        // Swallow a docstring we're collapsing — its text may be dedented to column 0,
        // which would otherwise look like the body ending.
        if let Some(q) = in_docstring {
            if trimmed.contains(q) {
                in_docstring = None;
            }
            continue;
        }

        // Always keep imports
        if import_re.is_match(trimmed) {
            sig_indent = None;
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Detect signature
        if sig_re.is_match(trimmed) {
            let indent = line.len() - line.trim_start().len();
            sig_indent = Some(indent);
            result.push_str(line);
            result.push('\n');
            // Emit placeholder
            let pad: String = " ".repeat(indent + 4);
            result.push_str(&pad);
            result.push_str("...\n");
            continue;
        }

        // If we're inside a body, skip lines more indented than the signature
        if let Some(base) = sig_indent {
            if trimmed.is_empty() {
                continue;
            }
            let indent = line.len() - line.trim_start().len();
            if indent > base {
                // A docstring can dedent its text below `base`; track it so those
                // lines stay skipped instead of being mistaken for the body's end.
                if let Some(q) = opens_docstring(trimmed) {
                    in_docstring = Some(q);
                }
                continue; // skip body
            }
            // Back to same or lesser indent — body ended
            sig_indent = None;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

/// Collapse do/end bodies (Elixir, Ruby).
fn collapse_do_end(content: &str, spec: &LangSpec) -> String {
    let sig_re = build_signature_regex(spec);
    let import_re = build_import_regex(spec);
    let mut result = String::with_capacity(content.len() / 2);
    let mut do_depth: i32 = 0;
    let mut in_body = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if import_re.is_match(trimmed) {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        if !in_body && sig_re.is_match(trimmed) {
            result.push_str(line);
            result.push('\n');
            if trimmed.ends_with("do") {
                in_body = true;
                do_depth = 1;
            }
            continue;
        }

        if !in_body {
            result.push_str(line);
            result.push('\n');
            continue;
        }

        // Count do/end nesting
        if trimmed.ends_with("do") || trimmed == "do" {
            do_depth += 1;
        }
        if trimmed == "end" || trimmed.starts_with("end") {
            do_depth -= 1;
            if do_depth <= 0 {
                result.push_str("    ...\n");
                result.push_str(line);
                result.push('\n');
                in_body = false;
                do_depth = 0;
            }
        }
    }

    result
}

// ── Helpers ─────────────────────────────────────────────────────────

/// If `trimmed` opens a Python docstring (`"""` / `'''`) that isn't closed on the
/// same line, return its quote style so the caller can skip to the close.
fn opens_docstring(trimmed: &str) -> Option<&'static str> {
    for q in ["\"\"\"", "'''"] {
        if let Some(rest) = trimmed.strip_prefix(q) {
            return if rest.contains(q) { None } else { Some(q) };
        }
    }
    None
}

fn build_signature_regex(spec: &LangSpec) -> Regex {
    let combined = spec.signature_patterns.join("|");
    Regex::new(&combined).unwrap_or_else(|_| Regex::new(r"^$").unwrap())
}

fn build_import_regex(spec: &LangSpec) -> Regex {
    if spec.import_patterns.is_empty() {
        return Regex::new(r"^\x00$").unwrap(); // never matches
    }
    let combined = spec.import_patterns.join("|");
    Regex::new(&combined).unwrap_or_else(|_| Regex::new(r"^\x00$").unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detect;

    fn rust_lang() -> LangId {
        LangId { name: "rust", spec: &detect::RUST }
    }

    fn python_lang() -> LangId {
        LangId { name: "python", spec: &detect::PYTHON }
    }

    #[test]
    fn rust_strip_comments() {
        let input = "// comment\nfn main() {\n    println!(\"hi\");\n}\n";
        let result = compress(input, &rust_lang(), Level::Full);
        assert!(!result.contains("// comment"));
        assert!(result.contains("fn main()"));
    }

    #[test]
    fn rust_keep_doc_comments() {
        let input = "/// Doc comment\nfn foo() {}\n";
        let result = compress(input, &rust_lang(), Level::Full);
        assert!(result.contains("/// Doc comment"));
    }

    #[test]
    fn python_collapse_body() {
        let input = "import os\n\ndef foo():\n    x = 1\n    return x\n\ndef bar():\n    pass\n";
        let result = compress(input, &python_lang(), Level::Ultra);
        assert!(result.contains("def foo():"));
        assert!(result.contains("def bar():"));
        assert!(!result.contains("x = 1"));
        assert!(result.contains("..."));
    }

    #[test]
    fn python_collapse_unindented_docstring() {
        // `"""\`-style docstring whose text sits at column 0 must not leak the body.
        let input = "def toposort(data):\n    \"\"\"\\\nDependencies are expressed as a dict.\nitems in the preceding sets.\"\"\"\n\n    if len(data) == 0:\n        return\n    return data\n\ndef other():\n    return 1\n";
        let result = compress(input, &python_lang(), Level::Ultra);
        assert!(result.contains("def toposort(data):"), "got:\n{result}");
        assert!(result.contains("def other():"), "got:\n{result}");
        assert!(!result.contains("Dependencies are expressed"), "docstring leaked:\n{result}");
        assert!(!result.contains("if len(data)"), "body leaked:\n{result}");
    }

    #[test]
    fn rust_collapse_body() {
        let input = "use std::io;\n\nfn main() {\n    let x = 1;\n    println!(\"{}\", x);\n}\n";
        let result = compress(input, &rust_lang(), Level::Ultra);
        assert!(result.contains("use std::io;"));
        assert!(result.contains("fn main() {"));
        assert!(result.contains("..."));
        assert!(!result.contains("let x = 1"));
    }

    #[test]
    fn preserves_imports() {
        let input = "use std::io;\nuse std::fs;\n\n// helper\nfn helper() {\n    todo!()\n}\n";
        let result = compress(input, &rust_lang(), Level::Ultra);
        assert!(result.contains("use std::io;"));
        assert!(result.contains("use std::fs;"));
    }

    fn count_tokens(s: &str) -> usize {
        s.split_whitespace().count()
    }

    #[test]
    fn meaningful_savings_on_real_code() {
        let input = r#"
// Copyright 2024 Foo Corp
// Licensed under Apache 2.0

use std::collections::HashMap;
use std::io::{self, Read};

/// A thing that does stuff
pub struct Foo {
    bar: String,
    baz: i32,
}

impl Foo {
    /// Create a new Foo
    pub fn new(bar: String) -> Self {
        Self {
            bar,
            baz: 0,
        }
    }

    // Internal helper
    fn compute(&self) -> i32 {
        let mut sum = 0;
        for i in 0..self.baz {
            sum += i;
        }
        sum
    }
}

fn main() {
    let foo = Foo::new("hello".to_string());
    println!("{}", foo.compute());
}
"#;
        let result = compress(input, &rust_lang(), Level::Ultra);
        let savings = 100.0 - (count_tokens(&result) as f64 / count_tokens(input) as f64 * 100.0);
        assert!(savings > 30.0, "Expected >30% savings, got {:.1}%", savings);
    }
}
