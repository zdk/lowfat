use anyhow::Result;
use lowfat_core::config::RunfConfig;

pub fn run() -> Result<()> {
    let config = RunfConfig::resolve();

    let session = if let Ok(db) = lowfat_core::db::Db::open(&config.data_dir) {
        db.session_summary(&today_utc()).ok()
    } else {
        None
    };

    let session = match session {
        Some(s) if s.commands > 0 => s,
        _ => return Ok(()),
    };

    let input = fmt_tokens(session.input_tokens);
    let output = fmt_tokens(session.output_tokens);

    // Estimate time without lowfat: output is smaller so agent processes it faster.
    // Use ratio of tokens as a rough proxy for time savings.
    let time_s = session.total_time_ms as f64 / 1000.0;
    let ratio = if session.output_tokens > 0 {
        session.input_tokens as f64 / session.output_tokens as f64
    } else {
        1.0
    };
    let time_without = time_s * ratio;

    const G: &str = "\x1b[32m";
    const Y: &str = "\x1b[33m";
    const D: &str = "\x1b[2m";
    const B: &str = "\x1b[1m";
    const R: &str = "\x1b[0m";

    println!();
    println!("  {Y}💰{R} {D}saved:{R} {B}{input}{R} {D}→{R} {G}{B}{output}{R} {D}tokens{R}");
    println!("  {Y}⚡{R} {D}speed:{R} {B}{time_without:.1}s{R} {D}→{R} {G}{B}{time_s:.1}s{R}");
    println!();

    Ok(())
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        format!("{n}")
    }
}

/// Get today's date as YYYY-MM-DD in UTC, using only std.
fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;

    let z = days as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}")
}
