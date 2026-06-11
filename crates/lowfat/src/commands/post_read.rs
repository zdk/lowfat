//! PostToolUse hook for Claude Code Read tool.
//! Reads hook JSON from stdin, compresses file content, outputs hook response.

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::Read;

pub fn run() -> Result<()> {
    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("Failed to read stdin")?;

    let payload: Value =
        serde_json::from_str(&input).context("Failed to parse PostToolUse JSON")?;

    let tool_output = match payload["tool_output"].as_str() {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(()), // no content or empty — pass through
    };

    let file_path = payload["tool_input"]["file_path"].as_str().unwrap_or("");
    if file_path.is_empty() {
        return Ok(());
    }

    // Map lowfat-core Level to lowfat-compress Level
    let core_level = lowfat_core::config::RunfConfig::resolve().level;
    let level = match core_level {
        lowfat_core::level::Level::Lite => lowfat_compress::Level::Lite,
        lowfat_core::level::Level::Full => lowfat_compress::Level::Full,
        lowfat_core::level::Level::Ultra => lowfat_compress::Level::Ultra,
    };

    let compressed = lowfat_compress::compress(tool_output, file_path, level);

    // Only emit if we saved something meaningful
    if compressed.len() >= tool_output.len() * 9 / 10 {
        return Ok(()); // <10% savings, not worth rewriting
    }

    let output = json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "updatedToolOutput": compressed
        }
    });

    println!("{}", serde_json::to_string(&output)?);
    Ok(())
}
