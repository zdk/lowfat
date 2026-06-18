use anyhow::Result;
use lowfat_core::config::RunfConfig;
use lowfat_core::level::Level;

pub fn run(value: Option<&str>) -> Result<()> {
    match value {
        Some(v) => {
            let level: Level = v.parse().map_err(|e: String| anyhow::anyhow!(e))?;
            println!("lowfat: level set to {level}");
            println!("  (export LOWFAT_LEVEL={level} to persist in this shell)");
        }
        None => {
            let config = RunfConfig::resolve();
            println!("lowfat: level={}", config.level);
        }
    }
    Ok(())
}
