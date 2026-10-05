use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

// Plugin source baked into the binary so install needs no network or repo.
// Must live inside the crate (not the workspace root) so `cargo publish` ships
// it — see crates/lowfat-plugin/src/embedded.rs for the same constraint.

// OpenCode v1.x plugin: a default-exported function returning event-key hooks
// (e.g. "tool.execute.before").
const PLUGIN_TS: &str = include_str!("../../embedded/opencode/lowfat.ts");

// OpenCode v2 plugin: v2 requires the module to export `{id, setup}`, and its
// `session.context` event no longer carries a per-tool `execute`, so the
// rewrite goes through `ctx.tool.hook("execute.before")` (present in both
// 2.0.12 and 2.0.16). Plain .js (no build step): the file ships verbatim.
const PLUGIN_V2_JS: &str = include_str!("../../embedded/opencode-v2/index.js");

fn home_dir() -> Option<PathBuf> {
    env::var("HOME")
        .or_else(|_| env::var("USERPROFILE")) // Windows
        .ok()
        .map(PathBuf::from)
}

fn config_home() -> Result<PathBuf> {
    // Treat an empty $XDG_CONFIG_HOME as unset (fall back to ~/.config).
    match env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty()) {
        Some(xdg) => Ok(PathBuf::from(xdg)),
        None => home_dir()
            .map(|h| h.join(".config"))
            .context("cannot resolve config home (set $HOME or $XDG_CONFIG_HOME)"),
    }
}

/// `~/.config/opencode/plugins/lowfat.ts` — the v1.x plugin shape.
fn plugin_v1_path() -> Result<PathBuf> {
    Ok(config_home()?.join("opencode").join("plugins").join("lowfat.ts"))
}

/// `~/.config/opencode/plugins/lowfat-v2/index.js` — the v2 plugin shape.
/// A directory plugin so it can sit beside `lowfat.ts` in the same tree
/// without a name clash (a bare `lowfat.js` next to `lowfat.ts` would be
/// ambiguous to a server that loads both).
fn plugin_v2_dir() -> Result<PathBuf> {
    Ok(config_home()?.join("opencode").join("plugins").join("lowfat-v2"))
}

/// Parse a major version out of an `opencode --version`-style banner:
/// "opencode v2.0.16" → 2, "1.14.3" → 1, no digit run → None.
fn parse_major_version(output: &str) -> Option<u64> {
    let start = output.find(|c: char| c.is_ascii_digit())?;
    let run: String = output[start..].chars().take_while(|c: &char| c.is_ascii_digit()).collect();
    run.parse().ok()
}

/// Best-effort detection of the OpenCode major version on PATH
/// (`opencode --version`). Returns None when the binary is absent or the
/// banner is unparseable — install then falls back to the v1.x plugin.
fn detect_opencode_major() -> Option<u64> {
    let out = std::process::Command::new("opencode")
        .arg("--version")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_major_version(&String::from_utf8_lossy(&out.stdout))
}

/// `lowfat opencode install` — write the plugin matching the OpenCode version
/// on PATH (v1.x → `lowfat.ts`, v2.x → `lowfat-v2/index.js`).
pub fn install() -> Result<()> {
    let major = detect_opencode_major();
    if major.filter(|m| *m >= 2).is_some() {
        let dir = plugin_v2_dir()?;
        fs::create_dir_all(&dir).with_context(|| format!("create {}", dir.display()))?;
        let path = dir.join("index.js");
        fs::write(&path, PLUGIN_V2_JS).with_context(|| format!("write {}", path.display()))?;
        println!("✓ Installed lowfat OpenCode v2 plugin → {}", path.display());
    } else {
        if major.is_none() {
            println!("  (note: `opencode --version` not found on PATH — installed the v1.x plugin;");
            println!("   if you are on OpenCode 2.x, re-run install from a shell that has it on PATH)");
        }
        let path = plugin_v1_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        fs::write(&path, PLUGIN_TS).with_context(|| format!("write {}", path.display()))?;
        println!("✓ Installed lowfat OpenCode plugin → {}", path.display());
    }
    println!("  Restart OpenCode, then run any command (e.g. `git status`).");
    Ok(())
}

/// `lowfat opencode uninstall` — remove whichever installed variants exist.
pub fn uninstall() -> Result<()> {
    let v1 = plugin_v1_path()?;
    let v2 = plugin_v2_dir()?;
    let mut removed = Vec::new();
    if v1.exists() {
        fs::remove_file(&v1).with_context(|| format!("remove {}", v1.display()))?;
        removed.push(v1.display().to_string());
    }
    if v2.exists() {
        fs::remove_dir_all(&v2).with_context(|| format!("remove {}", v2.display()))?;
        removed.push(v2.display().to_string());
    }
    if removed.is_empty() {
        println!("lowfat OpenCode plugin not installed (nothing to remove).");
    } else {
        for r in &removed {
            println!("✓ Removed lowfat OpenCode plugin: {}", r);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::parse_major_version;

    #[test]
    fn parses_v2_banner() {
        assert_eq!(parse_major_version("opencode v2.0.16"), Some(2));
    }

    #[test]
    fn parses_v1_banner() {
        assert_eq!(parse_major_version("opencode 1.14.3"), Some(1));
    }

    #[test]
    fn parses_bare_version() {
        assert_eq!(parse_major_version("2.0.16"), Some(2));
    }

    #[test]
    fn multi_digit_major() {
        assert_eq!(parse_major_version("OpenCode CLI v12.3.4-beta"), Some(12));
    }

    #[test]
    fn no_digits_is_none() {
        assert_eq!(parse_major_version("unknown"), None);
    }

    #[test]
    fn empty_is_none() {
        assert_eq!(parse_major_version(""), None);
    }
}
