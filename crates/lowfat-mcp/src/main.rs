//! lowfat-mcp — a Model Context Protocol server that exposes lowfat's output
//! filtering to MCP clients such as Claude Desktop.
//!
//! ## Why this exists
//!
//! lowfat's value is trimming verbose command output *before* it reaches an
//! agent. CLI agents (Claude Code, Codex) get that for free via the shell hook
//! or `shell-init`. GUI clients like Claude Desktop don't run a user shell, so
//! they have no place to hook. This server gives them one: a `run` tool that
//! executes a command and returns the lowfat-condensed output.
//!
//! ## How it works
//!
//! It speaks MCP over stdio (newline-delimited JSON-RPC 2.0) and, for each
//! `run` call, shells out to the `lowfat` binary — the same wrap-the-binary
//! approach used by the shell hook and the OpenCode plugin. The binary does
//! all filtering; this server is a thin, dependency-light bridge.
//!
//! Set `LOWFAT_BIN` to point at a non-default `lowfat` binary.

use std::io::{self, BufRead, Write};
use std::process::Command;

use serde_json::{json, Value};

const SERVER_NAME: &str = "lowfat-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Fallback when the client doesn't state a protocol version. We otherwise
/// echo back whatever the client requests during `initialize`.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";

fn main() {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => break, // EOF — client closed the pipe.
            Ok(_) => {}
            Err(e) => {
                eprintln!("[lowfat-mcp] stdin read error: {e}");
                break;
            }
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // No id is recoverable from an unparseable message.
                let _ = write_message(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &format!("parse error: {e}")),
                );
                continue;
            }
        };

        if let Some(response) = handle(&request) {
            if write_message(&mut stdout, &response).is_err() {
                break;
            }
        }
    }
}

fn write_message(out: &mut impl Write, msg: &Value) -> io::Result<()> {
    let serialized = serde_json::to_string(msg).expect("serializing a Value never fails");
    out.write_all(serialized.as_bytes())?;
    out.write_all(b"\n")?;
    out.flush()
}

/// Dispatch a JSON-RPC message. Returns `Some(response)` for requests and
/// `None` for notifications (which, per JSON-RPC, must not be answered).
fn handle(request: &Value) -> Option<Value> {
    let id = request.get("id").cloned();
    let method = request.get("method").and_then(Value::as_str).unwrap_or("");
    let is_notification = id.is_none();

    match method {
        "initialize" => Some(handle_initialize(id?, request)),
        "tools/list" => Some(success(id?, json!({ "tools": [run_tool_schema()] }))),
        "tools/call" => Some(handle_tools_call(id?, request)),
        "ping" => Some(success(id?, json!({}))),
        // Notifications (e.g. `notifications/initialized`) get no reply.
        _ if is_notification => None,
        _ => Some(error_response(
            id.unwrap_or(Value::Null),
            -32601,
            &format!("method not found: {method}"),
        )),
    }
}

fn handle_initialize(id: Value, request: &Value) -> Value {
    let protocol_version = request
        .get("params")
        .and_then(|p| p.get("protocolVersion"))
        .and_then(Value::as_str)
        .unwrap_or(DEFAULT_PROTOCOL_VERSION)
        .to_string();

    success(
        id,
        json!({
            "protocolVersion": protocol_version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        }),
    )
}

fn run_tool_schema() -> Value {
    json!({
        "name": "run",
        "description": "Execute a command and return its output condensed by lowfat. Use this for verbose, noisy commands (git, docker, ls, find, grep, build tools) to save tokens. Behaves like running the command directly, only the output is trimmed.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The program to run, e.g. \"git\", \"docker\", \"ls\"."
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments passed to the command, e.g. [\"status\"]."
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory to run the command in. Defaults to the server's working directory."
                },
                "level": {
                    "type": "string",
                    "enum": ["lite", "full", "ultra"],
                    "description": "Compression level for this call. lite = gentle, full = default, ultra = most aggressive."
                }
            },
            "required": ["command"]
        }
    })
}

fn handle_tools_call(id: Value, request: &Value) -> Value {
    let params = request.get("params");
    let name = params
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let arguments = params
        .and_then(|p| p.get("arguments"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "run" => run_tool(id, &arguments),
        other => error_response(id, -32602, &format!("unknown tool: {other}")),
    }
}

fn run_tool(id: Value, args: &Value) -> Value {
    let command = match args.get("command").and_then(Value::as_str) {
        Some(c) if !c.is_empty() => c,
        _ => return tool_error(id, "`command` is required and must be a non-empty string"),
    };

    let cmd_args: Vec<&str> = args
        .get("args")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();

    let lowfat_bin = std::env::var("LOWFAT_BIN").unwrap_or_else(|_| "lowfat".to_string());
    let mut cmd = Command::new(&lowfat_bin);
    cmd.arg(command).args(&cmd_args);

    if let Some(cwd) = args.get("cwd").and_then(Value::as_str) {
        cmd.current_dir(cwd);
    }
    if let Some(level) = args.get("level").and_then(Value::as_str) {
        cmd.env("LOWFAT_LEVEL", level);
    }

    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            return tool_error(
                id,
                &format!(
                    "failed to run `{lowfat_bin} {command}`: {e}. \
                     Is lowfat installed and on PATH? Set LOWFAT_BIN to override the binary path."
                ),
            )
        }
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(-1);

    let mut text = stdout.into_owned();
    if code != 0 {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("[exit code {code}]"));
        let stderr = stderr.trim_end();
        if !stderr.is_empty() {
            text.push('\n');
            text.push_str(stderr);
        }
    }

    success(
        id,
        json!({
            "content": [ { "type": "text", "text": text } ],
            "isError": code != 0,
        }),
    )
}

fn success(id: Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// A tool-level failure. Per MCP, these are reported as a successful JSON-RPC
/// result carrying `isError: true`, so the model sees the message and can react
/// — rather than a protocol error, which it cannot.
fn tool_error(id: Value, message: &str) -> Value {
    success(
        id,
        json!({
            "content": [ { "type": "text", "text": message } ],
            "isError": true,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialize_echoes_protocol_version_and_reports_server_info() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": { "protocolVersion": "2024-11-05" }
        });
        let resp = handle(&req).expect("initialize is a request");
        assert_eq!(resp["result"]["protocolVersion"], "2024-11-05");
        assert_eq!(resp["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(resp["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn initialize_falls_back_to_default_protocol_version() {
        let req = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize" });
        let resp = handle(&req).unwrap();
        assert_eq!(resp["result"]["protocolVersion"], DEFAULT_PROTOCOL_VERSION);
    }

    #[test]
    fn tools_list_advertises_the_run_tool() {
        let req = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
        let resp = handle(&req).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "run");
        assert_eq!(tools[0]["inputSchema"]["required"][0], "command");
    }

    #[test]
    fn notifications_get_no_response() {
        let req = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle(&req).is_none());
    }

    #[test]
    fn unknown_method_returns_method_not_found() {
        let req = json!({ "jsonrpc": "2.0", "id": 9, "method": "does/not/exist" });
        let resp = handle(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[test]
    fn run_without_command_is_a_tool_error() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 3,
            "method": "tools/call",
            "params": { "name": "run", "arguments": {} }
        });
        let resp = handle(&req).unwrap();
        assert_eq!(resp["result"]["isError"], true);
    }

    #[test]
    fn unknown_tool_is_an_error() {
        let req = json!({
            "jsonrpc": "2.0",
            "id": 4,
            "method": "tools/call",
            "params": { "name": "nope", "arguments": {} }
        });
        let resp = handle(&req).unwrap();
        assert_eq!(resp["error"]["code"], -32602);
    }
}
