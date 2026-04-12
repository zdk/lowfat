use anyhow::Result;
use lowfat_core::config::RunfConfig;
use lowfat_core::db::Db;

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const WHITE: &str = "\x1b[97m";

fn fmt_tokens(n: f64) -> String {
    if n >= 1_000_000.0 {
        format!("{:.1}M", n / 1_000_000.0)
    } else if n >= 1_000.0 {
        format!("{:.1}K", n / 1_000.0)
    } else {
        format!("{:.0}", n)
    }
}

/// Plugin-candidate ranking. High score = called often, big output,
/// and lowfat isn't trimming it much yet.
pub fn run(limit: usize) -> Result<()> {
    let config = RunfConfig::resolve();
    let db = Db::open(&config.data_dir)?;
    let rows = db.history_ranking(limit)?;

    println!();
    println!("  {BOLD}{WHITE}lowfat{RESET} {DIM}plugin candidates{RESET}");
    println!("  {DIM}─────────────────────────────────────────────────────────{RESET}");
    println!();

    if rows.is_empty() {
        println!("  {DIM}No data yet. Run some commands through lowfat!{RESET}");
        println!();
        return Ok(());
    }

    println!(
        "  {DIM}{:>3}  {:<25} {:>5}  {:>9}  {:>8}  {:>6}{RESET}",
        "#", "command", "runs", "avg raw", "savings", "plugin"
    );

    for (i, r) in rows.iter().enumerate() {
        let rank = i + 1;
        let label = if r.subcommand.is_empty() {
            r.command.clone()
        } else {
            format!("{} {}", r.command, r.subcommand)
        };
        let save_color = if r.savings_pct >= 50.0 {
            DIM
        } else if r.plugin_ratio < 0.5 {
            MAGENTA
        } else {
            YELLOW
        };
        let plugin_mark = if r.plugin_ratio >= 0.99 {
            format!("{DIM}yes{RESET}")
        } else if r.plugin_ratio <= 0.01 {
            format!("{MAGENTA}no{RESET}")
        } else {
            format!("{YELLOW}{:.0}%{RESET}", r.plugin_ratio * 100.0)
        };
        println!(
            "  {BOLD}{:>3}{RESET}  {CYAN}{:<25}{RESET} {:>4}x  {:>9}  {save_color}{:>7.1}%{RESET}  {:>6}",
            rank,
            label,
            r.runs,
            fmt_tokens(r.avg_raw_tokens),
            r.savings_pct,
            plugin_mark,
        );
    }

    println!();
    println!(
        "  {DIM}ranked by runs × avg tokens × (1 − savings). {MAGENTA}no{RESET}{DIM}-plugin rows are the best targets — see docs/PLUGINS.md{RESET}"
    );
    println!();

    Ok(())
}
