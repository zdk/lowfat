//! Content type detection from file extension and heuristics.

use std::path::Path;

/// Detected content type for routing to the appropriate compressor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentType {
    Code(LangId),
    Markdown,
    Html,
    Data,
    Lock,
    Unknown,
}

/// Language identifier for code files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangId {
    pub name: &'static str,
    pub spec: &'static LangSpec,
}

/// Data-driven language specification. One entry per supported language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LangSpec {
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub doc_comment: Option<&'static str>,
    pub scope: ScopeStyle,
    pub signature_patterns: &'static [&'static str],
    pub import_patterns: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeStyle {
    Braces,
    Indentation,
    DoEnd,
    None,
}

// ── Language specs ──────────────────────────────────────────────────

pub(crate) static RUST: LangSpec = LangSpec {
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    doc_comment: Some("///"),
    scope: ScopeStyle::Braces,
    signature_patterns: &[
        r"^\s*(pub\s+)?(async\s+)?(fn|struct|enum|trait|impl|type|mod|const|static)\s+",
    ],
    import_patterns: &[r"^\s*use\s+"],
};

pub(crate) static PYTHON: LangSpec = LangSpec {
    line_comment: Some("#"),
    block_comment: Some(("\"\"\"", "\"\"\"")),
    doc_comment: Some("\"\"\""),
    scope: ScopeStyle::Indentation,
    signature_patterns: &[r"^\s*(async\s+)?(def|class)\s+"],
    import_patterns: &[r"^\s*(import|from)\s+"],
};

static GO: LangSpec = LangSpec {
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    doc_comment: Some("//"),
    scope: ScopeStyle::Braces,
    signature_patterns: &[r"^\s*(func|type|var|const)\s+"],
    import_patterns: &[r"^\s*import\s+"],
};

static ELIXIR: LangSpec = LangSpec {
    line_comment: Some("#"),
    block_comment: None,
    doc_comment: Some("@doc"),
    scope: ScopeStyle::DoEnd,
    signature_patterns: &[
        r"^\s*(def|defp|defmodule|defmacro|defguard|defstruct|defprotocol|defimpl)\s+",
    ],
    import_patterns: &[r"^\s*(import|alias|use|require)\s+"],
};

static SHELL: LangSpec = LangSpec {
    line_comment: Some("#"),
    block_comment: None,
    doc_comment: None,
    scope: ScopeStyle::None,
    signature_patterns: &[r"^\s*\w+\s*\(\)\s*\{", r"^\s*function\s+\w+"],
    import_patterns: &[r"^\s*(\.|source)\s+"],
};

static JAVASCRIPT: LangSpec = LangSpec {
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    doc_comment: Some("/**"),
    scope: ScopeStyle::Braces,
    signature_patterns: &[
        r"^\s*(export\s+)?(async\s+)?(function|class|interface|type|enum)\s+",
        r"^\s*(export\s+)?(const|let|var)\s+\w+\s*=",
    ],
    import_patterns: &[r"^\s*import\s+"],
};

static JAVA: LangSpec = LangSpec {
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    doc_comment: Some("/**"),
    scope: ScopeStyle::Braces,
    signature_patterns: &[
        r"^\s*(public|private|protected|static|abstract|final|\s)*\s*(class|interface|enum|record)\s+",
        r"^\s*(public|private|protected|static|abstract|final|\s)*\s*\w+\s+\w+\s*\(",
    ],
    import_patterns: &[r"^\s*(import|package)\s+"],
};

static RUBY: LangSpec = LangSpec {
    line_comment: Some("#"),
    block_comment: Some(("=begin", "=end")),
    doc_comment: None,
    scope: ScopeStyle::DoEnd,
    signature_patterns: &[r"^\s*(def|class|module)\s+"],
    import_patterns: &[r"^\s*require\s+"],
};

static C_LANG: LangSpec = LangSpec {
    line_comment: Some("//"),
    block_comment: Some(("/*", "*/")),
    doc_comment: None,
    scope: ScopeStyle::Braces,
    signature_patterns: &[
        r"^\s*(static\s+|extern\s+|inline\s+)*\w+[\s*]+\w+\s*\(",
        r"^\s*(struct|enum|union|typedef)\s+",
    ],
    import_patterns: &[r"^\s*#\s*include\s+"],
};

// ── Extension → ContentType mapping ────────────────────────────────

pub fn detect(file_path: &str, _content: &str) -> ContentType {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let filename = Path::new(file_path)
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("");

    // Lock files (check filename first)
    if is_lock_file(filename, &ext) {
        return ContentType::Lock;
    }

    match ext.as_str() {
        // Code
        "rs" => ContentType::Code(LangId {
            name: "rust",
            spec: &RUST,
        }),
        "py" | "pyw" | "pyi" => ContentType::Code(LangId {
            name: "python",
            spec: &PYTHON,
        }),
        "go" => ContentType::Code(LangId {
            name: "go",
            spec: &GO,
        }),
        "ex" | "exs" => ContentType::Code(LangId {
            name: "elixir",
            spec: &ELIXIR,
        }),
        "sh" | "bash" | "zsh" | "fish" => ContentType::Code(LangId {
            name: "shell",
            spec: &SHELL,
        }),
        "js" | "jsx" | "mjs" | "cjs" => ContentType::Code(LangId {
            name: "javascript",
            spec: &JAVASCRIPT,
        }),
        "ts" | "tsx" | "mts" | "cts" => ContentType::Code(LangId {
            name: "typescript",
            spec: &JAVASCRIPT,
        }),
        "java" | "kt" | "kts" => ContentType::Code(LangId {
            name: "java",
            spec: &JAVA,
        }),
        "rb" => ContentType::Code(LangId {
            name: "ruby",
            spec: &RUBY,
        }),
        "c" | "h" => ContentType::Code(LangId {
            name: "c",
            spec: &C_LANG,
        }),
        "cpp" | "cc" | "cxx" | "hpp" | "hh" => ContentType::Code(LangId {
            name: "cpp",
            spec: &C_LANG,
        }),
        "swift" => ContentType::Code(LangId {
            name: "swift",
            spec: &C_LANG,
        }),
        "cs" => ContentType::Code(LangId {
            name: "csharp",
            spec: &C_LANG,
        }),

        // Markdown
        "md" | "markdown" | "mdx" => ContentType::Markdown,

        // HTML-like
        "html" | "htm" | "vue" | "svelte" => ContentType::Html,

        // Data/config
        "json" | "jsonc" | "json5" => ContentType::Data,
        "yaml" | "yml" => ContentType::Data,
        "toml" => ContentType::Data,
        "xml" | "svg" => ContentType::Data,
        "csv" | "tsv" => ContentType::Data,
        "env" => ContentType::Data,

        _ => ContentType::Unknown,
    }
}

fn is_lock_file(filename: &str, ext: &str) -> bool {
    matches!(
        filename,
        "Cargo.lock"
            | "package-lock.json"
            | "yarn.lock"
            | "pnpm-lock.yaml"
            | "Gemfile.lock"
            | "poetry.lock"
            | "Pipfile.lock"
            | "composer.lock"
            | "mix.lock"
    ) || ext == "lock"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_rust() {
        assert!(matches!(detect("src/main.rs", ""), ContentType::Code(_)));
    }

    #[test]
    fn detect_python() {
        assert!(matches!(detect("app.py", ""), ContentType::Code(_)));
    }

    #[test]
    fn detect_markdown() {
        assert_eq!(detect("README.md", ""), ContentType::Markdown);
    }

    #[test]
    fn detect_lock_file() {
        assert_eq!(detect("Cargo.lock", ""), ContentType::Lock);
        assert_eq!(detect("package-lock.json", ""), ContentType::Lock);
    }

    #[test]
    fn detect_json() {
        assert_eq!(detect("config.json", ""), ContentType::Data);
    }

    #[test]
    fn detect_html() {
        assert_eq!(detect("index.html", ""), ContentType::Html);
    }

    #[test]
    fn detect_unknown() {
        assert_eq!(detect("file.xyz", ""), ContentType::Unknown);
    }
}
