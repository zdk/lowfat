use anyhow::Result;
use lowfat_core::config::RunfConfig;
use lowfat_core::db::Db;

// ANSI color helpers
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const WHITE: &str = "\x1b[97m";

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn savings_color(pct: f64) -> &'static str {
    if pct >= 70.0 {
        GREEN
    } else if pct >= 40.0 {
        YELLOW
    } else {
        MAGENTA
    }
}

/// Render a compact bar showing input → output ratio
fn bar(input: u64, output: u64, width: usize) -> String {
    if input == 0 {
        return " ".repeat(width);
    }
    let ratio = output as f64 / input as f64;
    let filled = ((ratio * width as f64).round() as usize).min(width);
    let saved = width - filled;
    format!(
        "{GREEN}{}{CYAN}{}{RESET}",
        "█".repeat(saved),
        "░".repeat(filled),
    )
}

pub fn run() -> Result<()> {
    let config = RunfConfig::resolve();
    let db = Db::open(&config.data_dir)?;
    let summary = db.gain_summary()?;

    // Header
    println!();
    println!("  {BOLD}{WHITE}lowfat{RESET} {DIM}token savings{RESET}");
    println!("  {DIM}─────────────────────────────────────────{RESET}");
    println!();

    if summary.commands == 0 {
        println!("  {DIM}No data yet. Run some commands through lowfat!{RESET}");
        println!();
        return Ok(());
    }

    // Summary stats
    let color = savings_color(summary.savings_pct);
    println!("  {DIM}commands{RESET}  {BOLD}{}{RESET}", summary.commands,);
    println!(
        "  {DIM}input{RESET}    {BOLD}{}{RESET} {DIM}tokens{RESET}",
        fmt_tokens(summary.input_tokens),
    );
    println!(
        "  {DIM}output{RESET}   {BOLD}{}{RESET} {DIM}tokens{RESET}",
        fmt_tokens(summary.output_tokens),
    );
    println!(
        "  {DIM}saved{RESET}    {color}{BOLD}{}{RESET} {DIM}tokens{RESET}  {color}{BOLD}{:.1}%{RESET}",
        fmt_tokens(summary.saved_tokens),
        summary.savings_pct,
    );
    println!();
    println!(
        "           {}",
        bar(summary.input_tokens, summary.output_tokens, 30)
    );
    println!("           {GREEN}█ saved{RESET}  {CYAN}░ kept{RESET}");
    println!();

    // Top commands
    let top = db.top_commands(10)?;
    if !top.is_empty() {
        println!("  {BOLD}{WHITE}top commands{RESET}");
        println!("  {DIM}─────────────────────────────────────────{RESET}");

        // Find max saved for relative bar scaling
        let max_saved = top.iter().map(|c| c.saved).max().unwrap_or(1).max(1);

        for cmd in &top {
            let pct_color = savings_color(cmd.avg_pct);
            let bar_width = ((cmd.saved as f64 / max_saved as f64) * 16.0).round() as usize;
            let bar_str = format!("{GREEN}{}{RESET}", "█".repeat(bar_width.max(1)));

            println!(
                "  {CYAN}{:25}{RESET} {DIM}{:>4}x{RESET}  {bar_str} {pct_color}{BOLD}{:>5.1}%{RESET}",
                cmd.command,
                cmd.runs,
                cmd.avg_pct,
            );
        }
        println!();
    }

    Ok(())
}
