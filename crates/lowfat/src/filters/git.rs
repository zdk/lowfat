//! Native git filter — compact status, diff, log, show output.

use anyhow::Result;
use lowfat_core::level::Level;
use lowfat_plugin::plugin::{FilterInput, FilterOutput, FilterPlugin, PluginInfo};

pub struct GitFilter;

impl FilterPlugin for GitFilter {
    fn info(&self) -> PluginInfo {
        PluginInfo {
            name: "git-compact".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            commands: vec!["git".into()],
            // The first four have dedicated filter arms below; the rest are
            // listed so `lowfat history` breaks them out by subcommand instead
            // of collapsing them under bare `git`. They fall through to the
            // generic head_nonblank handler, which is fine for typically-short
            // output like `git add` or `git commit`.
            subcommands: vec![
                "status".into(),
                "log".into(),
                "diff".into(),
                "show".into(),
                "add".into(),
                "commit".into(),
                "checkout".into(),
                "switch".into(),
                "restore".into(),
                "branch".into(),
                "merge".into(),
                "rebase".into(),
                "reset".into(),
                "revert".into(),
                "cherry-pick".into(),
                "stash".into(),
                "tag".into(),
                "fetch".into(),
                "pull".into(),
                "push".into(),
                "clone".into(),
                "remote".into(),
                "init".into(),
                "config".into(),
                "blame".into(),
                "reflog".into(),
                "describe".into(),
                "rm".into(),
                "mv".into(),
                "clean".into(),
                "bisect".into(),
                "grep".into(),
            ],
        }
    }

    fn filter(&self, input: &FilterInput) -> Result<FilterOutput> {
        let text = match input.subcommand.as_str() {
            "status" => filter_status(&input.raw, input.level),
            "log" => filter_log(&input.raw, input.level),
            "diff" => filter_diff(&input.raw, input.level),
            "show" => filter_show(&input.raw, input.level),
            _ => head_nonblank(&input.raw, input.level.head_limit(30)),
        };
        Ok(FilterOutput {
            passthrough: text.is_empty(),
            text,
        })
    }
}

fn filter_status(raw: &str, level: Level) -> String {
    let limit = match level {
        Level::Lite => 60,
        Level::Full => 30,
        Level::Ultra => 15,
    };

    let lines: Vec<&str> = raw
        .lines()
        .filter(|line| match level {
            // Ultra: only short-status file lines (e.g. " M src/main.rs")
            Level::Ultra => {
                let trimmed = line.trim_start();
                trimmed.len() >= 2
                    && trimmed.as_bytes().get(1).copied() == Some(b' ')
                    && is_status_char(trimmed.as_bytes()[0])
            }
            // Lite: status lines + context headers
            Level::Lite => {
                let trimmed = line.trim_start();
                is_status_line(trimmed)
                    || trimmed.starts_with("## ")
                    || trimmed.starts_with("On branch")
                    || trimmed.starts_with("Changes")
                    || trimmed.starts_with("Untracked")
            }
            // Full: status lines + branch header
            Level::Full => {
                let trimmed = line.trim_start();
                is_status_line(trimmed) || trimmed.starts_with("## ")
            }
        })
        .take(limit)
        .collect();

    if lines.is_empty() {
        "git status: clean".into()
    } else {
        lines.join("\n")
    }
}

fn filter_log(raw: &str, level: Level) -> String {
    match level {
        Level::Lite => take_lines(raw, 50),
        Level::Ultra => {
            raw.lines()
                .filter(|l| l.starts_with("commit ") || l.starts_with("    "))
                .take(10)
                .collect::<Vec<_>>()
                .join("\n")
        }
        Level::Full => take_lines(raw, 25),
    }
}

fn filter_diff(raw: &str, level: Level) -> String {
    let (pattern_fn, limit): (fn(&str) -> bool, usize) = match level {
        Level::Lite => (is_diff_line, 400),
        Level::Ultra => (is_diff_header, 30),
        Level::Full => (is_diff_line, 200),
    };

    raw.lines()
        .filter(|l| pattern_fn(l))
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

fn filter_show(raw: &str, level: Level) -> String {
    match level {
        Level::Lite => {
            raw.lines()
                .filter(|l| !is_index_meta(l))
                .take(200)
                .collect::<Vec<_>>()
                .join("\n")
        }
        Level::Ultra => {
            raw.lines()
                .filter(|l| {
                    l.starts_with("commit ")
                        || l.starts_with("Author:")
                        || l.starts_with("Date:")
                        || l.starts_with("    ")
                        || l.starts_with("diff --git")
                        || (l.contains(" | ") && l.chars().any(|c| c == '+' || c == '-'))
                })
                .take(20)
                .collect::<Vec<_>>()
                .join("\n")
        }
        // Full: commit header + diff-content lines (same treatment as `git diff`).
        // Drops unchanged context, index/mode meta, --- / +++ path markers, blanks.
        Level::Full => {
            raw.lines()
                .filter(|l| is_commit_header(l) || is_diff_line(l))
                .take(100)
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

// --- helpers ---

fn is_status_char(b: u8) -> bool {
    matches!(b, b'M' | b'A' | b'D' | b'R' | b'C' | b'U' | b'?' | b'!')
}

fn is_status_line(s: &str) -> bool {
    s.len() >= 3 && is_status_char(s.as_bytes()[0]) && s.as_bytes()[1] == b' '
        || s.len() >= 4
            && s.as_bytes()[0] == b' '
            && is_status_char(s.as_bytes()[1])
            && s.as_bytes()[2] == b' '
}

fn is_diff_line(l: &str) -> bool {
    l.starts_with("diff ")
        || l.starts_with("--- ")
        || l.starts_with("+++ ")
        || l.starts_with("@@ ")
        || l.starts_with('+')
        || l.starts_with('-')
}

fn is_diff_header(l: &str) -> bool {
    l.starts_with("diff --git") || l.starts_with("@@ ")
}

fn is_commit_header(l: &str) -> bool {
    l.starts_with("commit ")
        || l.starts_with("Merge:")
        || l.starts_with("Author:")
        || l.starts_with("Date:")
        || l.starts_with("    ")
}

fn is_index_meta(l: &str) -> bool {
    l.starts_with("index ") || l.starts_with("mode ") || l.starts_with("similarity ")
}

fn head_nonblank(raw: &str, limit: usize) -> String {
    raw.lines()
        .filter(|l| !l.is_empty())
        .take(limit)
        .collect::<Vec<_>>()
        .join("\n")
}

fn take_lines(raw: &str, n: usize) -> String {
    raw.lines().take(n).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_clean() {
        let out = filter_status("", Level::Full);
        assert_eq!(out, "git status: clean");
    }

    #[test]
    fn status_modified() {
        let raw = " M src/main.rs\n M Cargo.toml\n";
        let out = filter_status(raw, Level::Full);
        assert!(out.contains("src/main.rs"));
        assert!(out.contains("Cargo.toml"));
    }

    #[test]
    fn diff_ultra_headers_only() {
        let raw = "diff --git a/f b/f\nindex abc..def\n--- a/f\n+++ b/f\n@@ -1 +1 @@\n-old\n+new\n";
        let out = filter_diff(raw, Level::Ultra);
        assert!(out.contains("diff --git"));
        assert!(out.contains("@@ "));
        assert!(!out.contains("-old"));
    }

    #[test]
    fn show_full_drops_context_and_meta() {
        let raw = "\
commit abc123
Author: zdk
Date:   Mon

    fix bug

diff --git a/f b/f
index abc..def 100644
--- a/f
+++ b/f
@@ -1,3 +1,3 @@
 unchanged context
-old line
+new line
 more context
";
        let out = filter_show(raw, Level::Full);
        assert!(out.contains("commit abc123"));
        assert!(out.contains("    fix bug"));
        assert!(out.contains("diff --git"));
        assert!(out.contains("-old line"));
        assert!(out.contains("+new line"));
        assert!(!out.contains("unchanged context"), "should drop context: {out}");
        assert!(!out.contains("index abc"), "should drop index meta: {out}");
    }

    #[test]
    fn log_ultra_compact() {
        let raw = "commit abc123\nAuthor: zdk\nDate: Mon\n\n    fix bug\n\ncommit def456\n";
        let out = filter_log(raw, Level::Ultra);
        assert!(out.contains("commit abc123"));
        assert!(out.contains("    fix bug"));
        // Author/Date stripped in ultra
        assert!(!out.contains("Author:"));
    }
}
