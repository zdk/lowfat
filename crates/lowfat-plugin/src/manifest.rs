use serde::Deserialize;
use std::path::Path;

/// Parsed `lowfat.toml` (or `init.toml`) plugin manifest.
#[derive(Debug, Deserialize)]
pub struct PluginManifest {
    pub plugin: PluginMeta,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    pub hooks: Option<HooksConfig>,
    pub pipeline: Option<PipelineConfig>,
}

#[derive(Debug, Deserialize)]
pub struct PluginMeta {
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub category: Option<String>,
    /// Which commands this plugin intercepts (e.g., ["git"])
    pub commands: Vec<String>,
    /// Optional: limit to specific subcommands
    pub subcommands: Option<Vec<String>>,
    /// Optional: real binary to exec when triggered via a shorthand command.
    /// Lets `commands = ["kubectl", "k"]` run `kubectl` even when invoked as
    /// `k` (which is a shell alias, not a binary on PATH).
    pub bin: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RuntimeConfig {
    /// Entrypoint relative to plugin dir. Omit it — see [`resolve_entry`].
    ///
    /// [`resolve_entry`]: RuntimeConfig::resolve_entry
    #[serde(default)]
    pub entry: Option<String>,
    /// Optional declared runtimes the plugin needs (python, uv, …).
    /// Used by `lowfat plugin doctor` to verify availability.
    #[serde(default)]
    pub requires: std::collections::BTreeMap<String, String>,
}

impl RuntimeConfig {
    /// Resolve the entrypoint filename for a plugin rooted at `base_dir`.
    ///
    /// An explicit `entry` always wins. Otherwise auto-detect: prefer
    /// `filter.lf` (the format for new plugins), falling back to `filter.sh`
    /// so pre-`.lf` shell plugins keep loading without a manifest change.
    pub fn resolve_entry(&self, base_dir: &Path) -> String {
        if let Some(entry) = &self.entry {
            return entry.clone();
        }
        if base_dir.join("filter.lf").is_file() {
            "filter.lf".to_string()
        } else {
            "filter.sh".to_string()
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct HooksConfig {
    pub on_install: Option<String>,
    pub on_update: Option<String>,
    pub on_remove: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PipelineConfig {
    pub pre: Option<Vec<String>>,
    pub post: Option<Vec<String>>,
}

impl PluginManifest {
    pub fn parse(content: &str) -> anyhow::Result<Self> {
        Ok(toml::from_str(content)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_manifest() {
        let toml = r#"
[plugin]
name = "git-compact"
commands = ["git"]

[runtime]
entry = "filter.sh"
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.name, "git-compact");
        assert_eq!(manifest.plugin.commands, vec!["git"]);
        assert_eq!(manifest.runtime.entry.as_deref(), Some("filter.sh"));
    }

    #[test]
    fn parse_minimal_manifest_no_runtime() {
        let toml = r#"
[plugin]
name = "git-compact"
commands = ["git"]
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.name, "git-compact");
        // No [runtime] → entry stays unset, resolved lazily at load time.
        assert!(manifest.runtime.entry.is_none());
    }

    #[test]
    fn resolve_entry_auto_detects() {
        let dir = std::env::temp_dir().join(format!("lowfat-resolve-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let rt = RuntimeConfig::default();
        // No filter.lf present → fall back to filter.sh.
        assert_eq!(rt.resolve_entry(&dir), "filter.sh");

        // filter.lf present → auto-detected.
        std::fs::write(dir.join("filter.lf"), "*:\n    head 30\n").unwrap();
        assert_eq!(rt.resolve_entry(&dir), "filter.lf");

        // Explicit entry always wins over auto-detection.
        let rt_explicit = RuntimeConfig {
            entry: Some("custom.sh".to_string()),
            ..Default::default()
        };
        assert_eq!(rt_explicit.resolve_entry(&dir), "custom.sh");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_full_manifest() {
        let toml = r#"
[plugin]
name = "git-compact"
version = "1.2.0"
description = "Compact git output for LLM contexts"
author = "zdk"
category = "git"
commands = ["git"]
subcommands = ["status", "diff", "log", "show"]

[runtime]
entry = "filter.sh"

[hooks]
on_install = "chmod +x filter.sh"

[pipeline]
pre = ["strip-ansi"]
post = ["truncate"]
"#;
        let manifest = PluginManifest::parse(toml).unwrap();
        assert_eq!(manifest.plugin.name, "git-compact");
        assert!(manifest.hooks.is_some());
        assert!(manifest.pipeline.is_some());
    }
}
