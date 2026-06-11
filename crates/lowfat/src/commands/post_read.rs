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

    process_payload(&input)
}

fn process_payload(input: &str) -> Result<()> {
    let payload: Value =
        serde_json::from_str(input).context("Failed to parse PostToolUse JSON")?;

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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_payload(file_path: &str, content: &str) -> String {
        json!({
            "tool_input": {"file_path": file_path},
            "tool_output": content
        })
        .to_string()
    }

    #[test]
    fn empty_tool_output_passes_through() {
        let payload = json!({"tool_input": {"file_path": "x.rs"}, "tool_output": ""}).to_string();
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn missing_file_path_passes_through() {
        let payload = json!({"tool_input": {}, "tool_output": "some content"}).to_string();
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn small_savings_not_emitted() {
        // Short content won't compress >10%
        let payload = make_payload("main.rs", "fn main() {}");
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn lock_file_compresses() {
        // Lock files get extreme summarization — guaranteed >10% savings
        let lock_content = (0..50)
            .map(|i| format!("[[package]]\nname = \"pkg-{}\"\nversion = \"1.0.{}\"\n", i, i))
            .collect::<String>();
        let payload = make_payload("Cargo.lock", &lock_content);
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn invalid_json_returns_error() {
        assert!(process_payload("not json").is_err());
    }
}
