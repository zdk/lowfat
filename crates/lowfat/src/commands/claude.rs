use anyhow::{Context, Result};
use lowfat_core::config::RunfConfig;
use std::fs;
use std::path::PathBuf;

// Bun preload that stops Claude Code's `agent_summary` side query (a 30 s
// "describe your last action in 3-5 words" call that resends each background
// agent's whole transcript). Baked in so install needs no network.
const PRELOAD_JS: &str = include_str!("../../embedded/claude/no-agent-summary.js");

/// `<lowfat home>/no-agent-summary.js`. Its presence is the switch:
/// `shell-init` exports `BUN_OPTIONS=--preload <path>` only when it exists.
pub fn preload_path() -> PathBuf {
    RunfConfig::resolve().home_dir.join("no-agent-summary.js")
}

/// `lowfat claude agent-summary [on|off]`
pub fn agent_summary(state: Option<&str>) -> Result<()> {
    let path = preload_path();
    match state {
        Some("off") => {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("create {}", parent.display()))?;
            }
            fs::write(&path, PRELOAD_JS).with_context(|| format!("write {}", path.display()))?;
            println!("✓ agent_summary off → {}", path.display());
            println!("  `lowfat shell-init` now exports BUN_OPTIONS=--preload for new shells.");
            println!(
                "  Restart: `claude daemon stop --any`, then open a new shell and start claude."
            );
            println!(
                "  Verify:  CC_AGENT_SUMMARY_LOG=/tmp/nas.log claude --version && cat /tmp/nas.log"
            );
            println!("  Note: unofficial; a Claude Code update can silently re-enable summaries.");
        }
        Some("on") => {
            if path.exists() {
                fs::remove_file(&path).with_context(|| format!("remove {}", path.display()))?;
                println!("✓ agent_summary on (removed {})", path.display());
                println!("  Open a new shell so BUN_OPTIONS is no longer exported.");
            } else {
                println!("agent_summary already on (nothing to remove).");
            }
        }
        Some(other) => anyhow::bail!("expected `on` or `off`, got `{other}`"),
        None => {
            let state = if path.exists() { "off" } else { "on" };
            println!("lowfat: claude agent_summary={state}");
            if path.exists() {
                println!("  preload: {}", path.display());
            }
        }
    }
    Ok(())
}
