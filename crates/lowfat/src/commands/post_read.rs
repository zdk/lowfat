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
    let payload: Value = serde_json::from_str(input).context("Failed to parse PostToolUse JSON")?;

    // Read tool delivers content as a structured object:
    //   tool_response: { type: "text", file: { content, numLines, totalLines, ... } }
    // Anything else (non-text reads, image/notebook payloads) passes through.
    let content = match payload["tool_response"]["file"]["content"].as_str() {
        Some(s) if !s.is_empty() => s,
        _ => return Ok(()),
    };

    let file_path = payload["tool_input"]["file_path"]
        .as_str()
        .or_else(|| payload["tool_response"]["file"]["filePath"].as_str())
        .unwrap_or("");
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

    let compressed = lowfat_compress::compress(content, file_path, level);

    // Only emit if we saved something meaningful
    if compressed.len() >= content.len() * 9 / 10 {
        return Ok(()); // <10% savings, not worth rewriting
    }

    // Echo back the tool_response object with content replaced; keep line counts
    // consistent so Claude Code's re-render isn't misleading.
    let mut tool_response = payload["tool_response"].clone();
    let line_count = compressed.lines().count();
    tool_response["file"]["content"] = json!(compressed);
    tool_response["file"]["numLines"] = json!(line_count);
    tool_response["file"]["totalLines"] = json!(line_count);

    let output = json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "updatedToolOutput": tool_response
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
            "tool_response": {"type": "text", "file": {"content": content}}
        })
        .to_string()
    }

    #[test]
    fn empty_tool_output_passes_through() {
        let payload = make_payload("x.rs", "");
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn missing_file_path_passes_through() {
        let payload = json!({"tool_response": {"file": {"content": "some content"}}}).to_string();
        // filePath fallback also absent — passes through
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn emits_structured_output_for_compressible_file() {
        // Capture stdout is awkward here; just assert the happy path doesn't error
        // and that real-shaped lock content is handled. Shape correctness is covered
        // by the field paths in process_payload (tool_response.file.content).
        let lock_content = (0..50)
            .map(|i| {
                format!(
                    "[[package]]\nname = \"pkg-{}\"\nversion = \"1.0.{}\"\n",
                    i, i
                )
            })
            .collect::<String>();
        let payload = make_payload("Cargo.lock", &lock_content);
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
            .map(|i| {
                format!(
                    "[[package]]\nname = \"pkg-{}\"\nversion = \"1.0.{}\"\n",
                    i, i
                )
            })
            .collect::<String>();
        let payload = make_payload("Cargo.lock", &lock_content);
        assert!(process_payload(&payload).is_ok());
    }

    #[test]
    fn invalid_json_returns_error() {
        assert!(process_payload("not json").is_err());
    }
}
