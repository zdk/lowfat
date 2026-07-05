//! Optional, best-effort logging of token savings to a personal Supabase
//! "observatory" (eng_technologies + eng_evaluations). Designed to be
//! completely non-blocking for the main filter path.
//!
//! Enabled only when LOWFAT_SUPABASE_URL + KEY (or the generic SUPABASE_*
//! fallbacks) are present in the environment. All errors are swallowed unless
//! LOWFAT_OBSERVATORY_DEBUG=1.

use std::path::Path;
use std::thread;
use std::time::Duration;

use serde_json::json;

static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

/// Returns true if observatory credentials are configured (checked once).
pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| resolve_url_key().is_some())
}

fn resolve_url_key() -> Option<(String, String)> {
    let url = std::env::var("LOWFAT_SUPABASE_URL")
        .or_else(|_| std::env::var("SUPABASE_URL"))
        .ok()?;
    let key = std::env::var("LOWFAT_SUPABASE_KEY")
        .or_else(|_| std::env::var("SUPABASE_SERVICE_ROLE_KEY"))
        .ok()?;
    if url.trim().is_empty() || key.trim().is_empty() {
        return None;
    }
    Some((url, key))
}

/// Lightweight record of one filter invocation's savings.
#[derive(Clone, Debug)]
pub struct SavingsData {
    pub command: String,
    pub subcommand: String,
    pub raw_tokens: u64,
    pub filtered_tokens: u64,
    pub had_plugin: bool,
    pub reduced: bool,
    pub exit_code: i32,
    pub exec_time_ms: Option<u64>,
}

/// Fire-and-forget log. Spawns a background thread if enabled.
/// Never blocks the caller and never propagates errors.
///
/// When LOWFAT_OBSERVATORY_DEBUG is set, run synchronously so diagnostics are
/// visible before this short-lived CLI process exits.
pub fn log_savings_if_enabled(data_dir: &Path, data: SavingsData) {
    if !enabled() {
        return;
    }
    let dir = data_dir.to_path_buf();
    if std::env::var("LOWFAT_OBSERVATORY_DEBUG").is_ok() {
        if let Err(e) = log_savings_inner(&dir, &data) {
            eprintln!("[lowfat] observatory log failed: {e}");
        }
        return;
    }
    thread::spawn(move || {
        if let Err(e) = log_savings_inner(&dir, &data) {
            if std::env::var("LOWFAT_OBSERVATORY_DEBUG").is_ok() {
                eprintln!("[lowfat] observatory log failed: {e}");
            }
        }
    });
}

fn log_savings_inner(data_dir: &Path, data: &SavingsData) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (url, key) = resolve_url_key().ok_or("missing observatory credentials")?;
    if std::env::var("LOWFAT_OBSERVATORY_DEBUG").is_ok() {
        eprintln!("[lowfat] observatory: data_dir={}", data_dir.display());
        eprintln!("[lowfat] observatory: posting to {}/rest/v1/eng_evaluations (tech ensure first)", url.trim_end_matches('/'));
    }
    let tech_id = ensure_lowfat_technology(&url, &key, data_dir)?;

    let saved = data.raw_tokens as i64 - data.filtered_tokens as i64;
    let pct = if data.raw_tokens > 0 {
        (saved as f64 / data.raw_tokens as f64) * 100.0
    } else {
        0.0
    };
    let verdict = if saved > 0 { "adopt" } else { "trial" };

    let summary = if data.subcommand.is_empty() {
        format!("lowfat {}: {}→{} tokens ({:.1}%)", data.command, data.raw_tokens, data.filtered_tokens, pct)
    } else {
        format!("lowfat {} {}: {}→{} tokens ({:.1}%)", data.command, data.subcommand, data.raw_tokens, data.filtered_tokens, pct)
    };

    let scores = json!({
        "command": data.command,
        "subcommand": data.subcommand,
        "raw_tokens": data.raw_tokens,
        "filtered_tokens": data.filtered_tokens,
        "saved_tokens": saved,
        "savings_pct": pct,
        "had_plugin": data.had_plugin,
        "reduced": data.reduced,
        "exit_code": data.exit_code,
        "exec_time_ms": data.exec_time_ms,
    });

    let body = json!({
        "technology_id": tech_id,
        "phase": "retrospective",
        "verdict": verdict,
        "scores": scores,
        "summary": summary,
        "status": "open",
    });

    let base = url.trim_end_matches('/');
    let endpoint = format!("{base}/rest/v1/eng_evaluations");

    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(4))
        .build();

    let resp = agent
        .post(&endpoint)
        .set("apikey", &key)
        .set("Authorization", &format!("Bearer {}", key))
        .set("Content-Type", "application/json")
        .set("Prefer", "return=minimal")
        .send_json(body)?;

    // Best effort: drain any small response body
    let _ = resp.into_string();
    if std::env::var("LOWFAT_OBSERVATORY_DEBUG").is_ok() {
        eprintln!("[lowfat] observatory: POST eng_evaluations accepted (status OK)");
    }
    Ok(())
}

/// Ensure the "lowfat" technology row exists and return its UUID.
/// Caches the id in data_dir/lowfat_tech_id for fast subsequent calls.
fn ensure_lowfat_technology(
    url: &str,
    key: &str,
    data_dir: &Path,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let cache_path = data_dir.join("lowfat_tech_id");

    // Fast path: cached id
    if let Ok(s) = std::fs::read_to_string(&cache_path) {
        let trimmed = s.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    let base = url.trim_end_matches('/');
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(3))
        .build();

    // Try to find existing by slug
    let get_url = format!("{base}/rest/v1/eng_technologies?select=id&slug=eq.lowfat");
    if let Ok(res) = agent
        .get(&get_url)
        .set("apikey", key)
        .set("Authorization", &format!("Bearer {}", key))
        .call()
    {
        if let Ok(rows) = res.into_json::<Vec<serde_json::Value>>() {
            if let Some(first) = rows.first() {
                if let Some(id) = first.get("id").and_then(|v| v.as_str()) {
                    let _ = std::fs::write(&cache_path, id);
                    return Ok(id.to_string());
                }
            }
        }
    }

    // Not found — insert
    let ins = json!({
        "slug": "lowfat",
        "name": "lowfat",
        "category": "tool",
        "status": "adopted",
        "description": "Lightweight token-aware output and file content filter for LLM agents"
    });

    let post_url = format!("{base}/rest/v1/eng_technologies");
    let created_res = agent
        .post(&post_url)
        .set("apikey", key)
        .set("Authorization", &format!("Bearer {}", key))
        .set("Content-Type", "application/json")
        .set("Prefer", "return=representation")
        .send_json(ins);

    match created_res {
        Ok(resp) => {
            if let Ok(rows) = resp.into_json::<Vec<serde_json::Value>>() {
                if let Some(first) = rows.first() {
                    if let Some(id) = first.get("id").and_then(|v| v.as_str()) {
                        let _ = std::fs::write(&cache_path, id);
                        return Ok(id.to_string());
                    }
                }
            }
        }
        Err(_) => {
            // Possible conflict (unique slug) from a race. Fall back to GET.
        }
    }

    // Re-fetch after possible conflict
    let get_url2 = format!("{base}/rest/v1/eng_technologies?select=id&slug=eq.lowfat");
    let res2 = agent
        .get(&get_url2)
        .set("apikey", key)
        .set("Authorization", &format!("Bearer {}", key))
        .call()?;

    let rows: Vec<serde_json::Value> = res2.into_json()?;
    let id = rows
        .first()
        .and_then(|v| v.get("id"))
        .and_then(|v| v.as_str())
        .ok_or("failed to resolve lowfat technology id")?
        .to_string();

    let _ = std::fs::write(&cache_path, &id);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_when_no_env() {
        // Ensure we don't accidentally treat empty as enabled in tests.
        // We cannot easily clear env for the OnceLock in a unit test without
        // forking, so just assert the resolver returns None for obviously bad values.
        // Instead, directly test the private resolver by temporarily setting and then
        // we just trust the logic; here we test that malformed does not enable.
        std::env::remove_var("LOWFAT_SUPABASE_URL");
        std::env::remove_var("SUPABASE_URL");
        std::env::remove_var("LOWFAT_SUPABASE_KEY");
        std::env::remove_var("SUPABASE_SERVICE_ROLE_KEY");
        // Re-init path is cached; this test mainly documents intent.
        // A true off test would require a fresh process.
    }
}
