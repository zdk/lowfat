use anyhow::Result;
use lowfat_core::config::{find_config, RunfConfig};

pub fn run() -> Result<()> {
    let config = RunfConfig::resolve();

    const B: &str = "\x1b[1m";
    const D: &str = "\x1b[2m";
    const G: &str = "\x1b[32m";
    const Y: &str = "\x1b[33m";
    const R: &str = "\x1b[0m";
    const RED: &str = "\x1b[31m";

    // Config file location
    match find_config() {
        Some(path) => println!("  {D}config:{R}  {G}{}{R}", path.display()),
        None => println!("  {D}config:{R}  {Y}none{R} {D}(no .lowfat file found){R}"),
    }

    // Level
    println!("  {D}level:{R}   {B}{}{R}", config.level);

    // Disabled filters
    if config.disabled.is_empty() {
        println!("  {D}disable:{R} {D}none{R}");
    } else {
        let mut disabled: Vec<_> = config.disabled.iter().collect();
        disabled.sort();
        println!(
            "  {D}disable:{R} {}",
            disabled
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Whitelist
    if let Some(ref allowed) = config.allowed {
        let mut filters: Vec<_> = allowed.iter().collect();
        filters.sort();
        println!(
            "  {D}filters:{R} {}",
            filters
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    // Pipelines
    if !config.pipelines.is_empty() {
        println!();
        println!("  {B}pipelines{R}");
        let mut cmds: Vec<_> = config.pipelines.keys().collect();
        cmds.sort();
        for cmd in cmds {
            let cp = &config.pipelines[cmd];
            if let Some(ref p) = cp.default {
                println!("    {D}pipeline.{R}{cmd} {D}={R} {}", p.display());
            }
            if let Some(ref p) = cp.on_error {
                println!("    {D}pipeline.{R}{cmd}{D}.error ={R} {}", p.display());
            }
            if let Some(ref p) = cp.on_empty {
                println!("    {D}pipeline.{R}{cmd}{D}.empty ={R} {}", p.display());
            }
            if let Some(ref p) = cp.on_large {
                println!("    {D}pipeline.{R}{cmd}{D}.large ={R} {}", p.display());
            }
        }
    }

    // Validate config file
    if let Some(path) = find_config() {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let mut warnings = Vec::new();
            for (i, line) in content.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let valid = line.starts_with("level=")
                    || line.starts_with("disable=")
                    || line.starts_with("filters=")
                    || line.starts_with("pipeline.");
                if !valid {
                    warnings.push(format!("line {}: unknown setting: {}", i + 1, line));
                }
                // Validate level value
                if let Some(val) = line.strip_prefix("level=") {
                    if !matches!(val, "lite" | "full" | "ultra") {
                        warnings.push(format!(
                            "line {}: invalid level '{}' (expected: lite, full, ultra)",
                            i + 1,
                            val
                        ));
                    }
                }
                // Validate pipeline has = and a spec
                if let Some(rest) = line.strip_prefix("pipeline.") {
                    if let Some((key, spec)) = rest.split_once('=') {
                        let key = key.trim();
                        let spec = spec.trim();
                        if key.is_empty() {
                            warnings.push(format!("line {}: pipeline missing command name", i + 1));
                        }
                        if spec.is_empty() {
                            warnings.push(format!(
                                "line {}: pipeline.{} has empty spec",
                                i + 1,
                                key
                            ));
                        }
                        // Check condition suffix
                        if let Some((_, cond)) = key.split_once('.') {
                            if !matches!(
                                cond,
                                "error"
                                    | "empty"
                                    | "large"
                                    | "diff"
                                    | "status"
                                    | "log"
                                    | "show"
                                    | "build"
                                    | "test"
                                    | "check"
                                    | "clippy"
                                    | "run"
                                    | "install"
                                    | "audit"
                                    | "ps"
                                    | "images"
                            ) {
                                // Only warn for known condition suffixes that look wrong
                                // This is a heuristic — subcommand names are open-ended
                            }
                        }
                    } else {
                        warnings.push(format!("line {}: pipeline.{} missing '='", i + 1, rest));
                    }
                }
            }

            if !warnings.is_empty() {
                println!();
                println!("  {RED}{B}warnings{R}");
                for w in &warnings {
                    println!("    {RED}!{R} {w}");
                }
            }
        }
    }

    // Paths
    println!();
    println!("  {D}home:{R}    {}", config.home_dir.display());
    println!("  {D}plugins:{R} {}", config.plugin_dir.display());
    println!("  {D}data:{R}    {}", config.data_dir.display());

    Ok(())
}
