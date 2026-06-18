use anyhow::{anyhow, Context, Result};
use lowfat_core::level::Level;
use lowfat_core::lf::{self, ExecCtx, ExplainTrace, RuleSet};
use lowfat_core::tokens::estimate_tokens;
use std::io::{Read, Write};

/// `lowfat filter <path.lf>` — run a .lf rule file against stdin and
/// write filtered output to stdout. Standalone tool for plugin authors
/// to test rules against captured samples without going through the
/// hook / shell-init path.
///
/// With `--explain`, per-stage diagnostics are written to stderr while
/// the filtered output still goes to stdout, so the command remains
/// usable in pipelines.
pub fn run(
    path: &str,
    sub: &str,
    level_str: &str,
    args_str: &str,
    exit_code: i32,
    explain: bool,
) -> Result<()> {
    // `lf::load` reads the file and resolves any `include` directives.
    let rs = lf::load(std::path::Path::new(path)).with_context(|| format!("parsing {path}"))?;
    let level: Level = level_str
        .parse()
        .map_err(|e: String| anyhow!("invalid --level: {e}"))?;
    let args: Vec<String> = if args_str.is_empty() {
        vec![]
    } else {
        args_str.split_whitespace().map(|s| s.to_string()).collect()
    };

    let mut input = String::new();
    std::io::stdin()
        .read_to_string(&mut input)
        .context("reading stdin")?;

    let ctx = ExecCtx {
        sub,
        level,
        exit_code,
        args: &args,
    };

    if explain {
        let (out, trace) =
            lf::execute_explain(&rs, &ctx, &input).with_context(|| format!("executing {path}"))?;
        print_explain(&rs, &ctx, &trace, &input, &out);
        let mut stdout = std::io::stdout();
        stdout.write_all(out.as_bytes())?;
    } else {
        let out = lf::execute(&rs, &ctx, &input).with_context(|| format!("executing {path}"))?;
        let mut stdout = std::io::stdout();
        stdout.write_all(out.as_bytes())?;
    }
    Ok(())
}

fn print_explain(rs: &RuleSet, ctx: &ExecCtx, trace: &ExplainTrace, input: &str, output: &str) {
    let mut stderr = std::io::stderr();

    let Some(idx) = trace.matched_rule else {
        let _ = writeln!(
            stderr,
            "[explain] no rule matched (sub={}, level={})",
            ctx.sub, ctx.level
        );
        return;
    };
    let rule = &rs.rules[idx];
    let _ = writeln!(
        stderr,
        "[explain] match: {} (line {})",
        describe_selector(rule),
        rule.line_no
    );
    for s in &trace.stages {
        let _ = writeln!(
            stderr,
            "  -> {:<32} stdin: {:>4}l / {:>5}B  stdout: {:>4}l / {:>5}B  {:>5}µs",
            truncate(&s.op_desc, 32),
            s.stdin_lines,
            s.stdin_bytes,
            s.stdout_lines,
            s.stdout_bytes,
            s.elapsed_us,
        );
    }
    let raw_t = estimate_tokens(input);
    let out_t = estimate_tokens(output);
    let pct = if raw_t > 0 {
        100.0 - (out_t as f64 / raw_t as f64) * 100.0
    } else {
        0.0
    };
    let _ = writeln!(
        stderr,
        "[explain] result: {}B / {}t -> {}B / {}t ({:.1}% saved)",
        input.len(),
        raw_t,
        output.len(),
        out_t,
        pct
    );
}

fn describe_selector(rule: &lowfat_core::lf::Rule) -> String {
    use lowfat_core::lf::{LevelPattern, SubPattern};
    let sub = match &rule.sub {
        SubPattern::Star => "*".to_string(),
        SubPattern::Alt(a) => a.join("|"),
    };
    let lvl = match &rule.level {
        LevelPattern::Star => "*".to_string(),
        LevelPattern::Specific(l) => l.to_string(),
    };
    format!("{sub}, {lvl}")
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n - 1).collect();
        out.push('…');
        out
    }
}
