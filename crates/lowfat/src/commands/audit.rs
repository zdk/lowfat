use anyhow::Result;
use lowfat_core::config::RunfConfig;
use lowfat_core::db::Db;

pub fn run(limit: usize) -> Result<()> {
    let config = RunfConfig::resolve();
    let db = Db::open(&config.data_dir)?;

    let entries = db.audit_log(limit)?;

    if entries.is_empty() {
        println!("No audit entries yet.");
        return Ok(());
    }

    println!("Recent plugin activity:");
    println!(
        "  {:20} {:20} {:8} {:10} {:12} details",
        "timestamp", "plugin", "runtime", "command", "action"
    );
    println!("  {}", "-".repeat(90));

    for entry in &entries {
        println!(
            "  {:20} {:20} {:8} {:10} {:12} {}",
            entry.timestamp,
            entry.plugin_name,
            entry.runtime_type,
            entry.command,
            entry.action,
            if entry.details.is_empty() {
                "-"
            } else {
                &entry.details
            }
        );
    }

    Ok(())
}
