use crate::filters;
use lowfat_core::config::RunfConfig;
use lowfat_core::db::{Db, InvocationRecord, TrackRecord};
use lowfat_core::pipeline::{Pipeline, StageType};
use lowfat_core::tee;
use lowfat_plugin::discovery::{discover_plugins, resolve_plugins, DiscoveredPlugin};
use lowfat_plugin::plugin::{FilterInput, FilterPlugin};
use lowfat_runner::runner::{exec_command, execute_pipeline, HybridRunner};
use std::collections::HashMap;
use std::time::Instant;

/// Main filter path: execute command, apply pipeline, track metrics.
/// Returns the process exit code.
pub fn run(args: &[String]) -> i32 {
    let cmd = &args[0];
    let cmd_args: Vec<String> = args[1..].to_vec();
    let subcommand = cmd_args.first().cloned().unwrap_or_default();

    let config = RunfConfig::resolve();

    if !config.is_enabled(cmd) {
        return passthrough(cmd, &cmd_args, &config);
    }

    // Native built-in filters (git, docker, ls)
    let all_filters = filters::builtins();
    // External plugins from ~/.lowfat/plugins/ (user-installed, runtime-loaded)
    let external_plugins = discover_plugins(&config.plugin_dir);
    let external_map = resolve_plugins(&external_plugins);

    // Execute the real command
    let start = Instant::now();
    let (raw, exit_code) = match exec_command(cmd, &cmd_args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lowfat] exec error: {e}");
            return 1;
        }
    };

    // Resolve which filter/pipeline to use
    let filter_name = resolve_filter_name(cmd, &all_filters, &external_plugins, &external_map);
    let pipeline = resolve_pipeline(cmd, exit_code, &raw, &config, &filter_name,
        &external_plugins, &external_map);

    // Build plugin map for pipeline execution: builtins + loaded community plugins
    let mut plugin_map: HashMap<String, Box<dyn FilterPlugin>> = all_filters;
    for stage in &pipeline.stages {
        if stage.stage_type == StageType::Plugin
            && !plugin_map.contains_key(&stage.name)
        {
            if let Some(loaded) = load_external_plugin(&stage.name, &external_plugins, &external_map) {
                plugin_map.insert(stage.name.clone(), loaded);
            }
        }
    }

    let input = FilterInput {
        raw: raw.clone(),
        command: cmd.clone(),
        subcommand: subcommand.clone(),
        args: cmd_args.clone(),
        level: config.level,
        head_limit: config.level.head_limit(40),
        exit_code,
    };

    // Execute the pipeline chain
    let filtered = match execute_pipeline(&pipeline, &raw, &input, &plugin_map) {
        Ok(text) => text,
        Err(_) => raw.clone(),
    };

    let elapsed = start.elapsed().as_millis() as u64;

    // Track
    if let Ok(db) = Db::open(&config.data_dir) {
        let args_str = cmd_args.join(" ");
        let pipeline_desc = pipeline.display();
        let _ = db.track(&TrackRecord {
            original_cmd: format!("{cmd} {args_str}"),
            lowfat_cmd: format!("lowfat[{pipeline_desc}] {cmd} {args_str}"),
            raw: raw.clone(),
            filtered: filtered.clone(),
            exec_time_ms: elapsed,
            project_path: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        });
        // Usage history — command+subcommand only, no args. Powers `lowfat history`.
        let _ = db.record_invocation(&InvocationRecord {
            command: cmd.clone(),
            subcommand: history_subcommand(&subcommand),
            raw_tokens: lowfat_core::tokens::estimate_tokens(&raw) as u64,
            filtered_tokens: lowfat_core::tokens::estimate_tokens(&filtered) as u64,
            had_plugin: filter_name.is_some(),
            exit_code,
        });
    }

    // Tee on failure
    let tee_dir = config.data_dir.join("tee");
    tee::save_on_failure(&tee_dir, &format!("{cmd}_{subcommand}"), &raw, exit_code);

    print!("{filtered}");
    exit_code
}

/// Find filter name for a command: native builtins first, then community plugins.
fn resolve_filter_name(
    cmd: &str,
    builtins: &HashMap<String, Box<dyn FilterPlugin>>,
    plugins: &[DiscoveredPlugin],
    cmd_map: &HashMap<String, usize>,
) -> Option<String> {
    // Native builtin? The map key is the command name, filter name is "{cmd}-compact"
    if builtins.contains_key(cmd) {
        return Some(cmd.to_string());
    }
    // Community plugin?
    if let Some(&idx) = cmd_map.get(cmd) {
        return Some(plugins[idx].manifest.plugin.name.clone());
    }
    None
}

/// Resolve which pipeline to use for a command.
fn resolve_pipeline(
    cmd: &str,
    exit_code: i32,
    raw: &str,
    config: &RunfConfig,
    filter_name: &Option<String>,
    plugins: &[DiscoveredPlugin],
    cmd_map: &HashMap<String, usize>,
) -> Pipeline {
    // 1. Check .lowfat conditional pipelines
    if let Some(conditional) = config.pipeline_for(cmd) {
        if let Some(pipeline) = conditional.select(exit_code, raw) {
            return pipeline.clone();
        }
    }

    // 2. Check community plugin init.toml pre/post processors
    if let Some(&idx) = cmd_map.get(cmd) {
        let manifest = &plugins[idx].manifest;
        let name = &manifest.plugin.name;
        if let Some(ref pipe_config) = manifest.pipeline {
            let pre = pipe_config.pre.as_deref().unwrap_or(&[]);
            let post = pipe_config.post.as_deref().unwrap_or(&[]);
            if !pre.is_empty() || !post.is_empty() {
                return Pipeline::from_parts(pre, name, post);
            }
        }
    }

    // 3. Single filter (builtin or community)
    if let Some(ref name) = filter_name {
        return Pipeline::single(name);
    }

    // No filter found
    Pipeline::parse("passthrough")
}

/// Load a community plugin by name.
fn load_external_plugin(
    name: &str,
    plugins: &[DiscoveredPlugin],
    cmd_map: &HashMap<String, usize>,
) -> Option<Box<dyn FilterPlugin>> {
    for plugin in plugins {
        if plugin.manifest.plugin.name == name {
            return HybridRunner::load(plugin).ok();
        }
    }
    if let Some(&idx) = cmd_map.get(name) {
        return HybridRunner::load(&plugins[idx]).ok();
    }
    None
}

/// Normalise the first arg into a subcommand for history. Flags (e.g. `-la`)
/// are not subcommands, so commands like `ls -la` are grouped under `""`.
fn history_subcommand(first: &str) -> String {
    if first.starts_with('-') { String::new() } else { first.to_string() }
}

/// Run command unfiltered, still track as passthrough.
fn passthrough(cmd: &str, args: &[String], config: &RunfConfig) -> i32 {
    let start = Instant::now();
    let (raw, exit_code) = match exec_command(cmd, args) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[lowfat] exec error: {e}");
            return 1;
        }
    };

    let elapsed = start.elapsed().as_millis() as u64;

    if let Ok(db) = Db::open(&config.data_dir) {
        let args_str = args.join(" ");
        let _ = db.track(&TrackRecord {
            original_cmd: format!("{cmd} {args_str}"),
            lowfat_cmd: format!("lowfat:passthrough {cmd} {args_str}"),
            raw: raw.clone(),
            filtered: raw.clone(),
            exec_time_ms: elapsed,
            project_path: std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_default(),
        });
        let subcommand = args.first().cloned().unwrap_or_default();
        let tokens = lowfat_core::tokens::estimate_tokens(&raw) as u64;
        let _ = db.record_invocation(&InvocationRecord {
            command: cmd.to_string(),
            subcommand: history_subcommand(&subcommand),
            raw_tokens: tokens,
            filtered_tokens: tokens,
            had_plugin: false,
            exit_code,
        });
    }

    print!("{raw}");
    exit_code
}
