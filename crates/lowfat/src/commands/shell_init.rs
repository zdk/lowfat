use anyhow::Result;

/// Print shell init script for eval.
/// Usage: eval "$(lowfat shell-init zsh)"
///
/// The generated script queries `lowfat filters --commands` at eval time
/// to discover all available filters (builtins + plugins + pipelines),
/// then creates wrapper functions for each command.
pub fn run(shell: &str) -> Result<()> {
    match shell {
        "zsh" | "bash" => print_posix_init(),
        "fish" => print_fish_init(),
        _ => {
            eprintln!("lowfat: unsupported shell: {shell} (supported: bash, zsh, fish)");
        }
    }
    Ok(())
}

/// `export BUN_OPTIONS=--preload <path>` when `lowfat claude agent-summary off`
/// is active. Outside the agent-env guard: it must reach the shell that
/// launches `claude`. Idempotent so nested shells don't append twice.
fn posix_preload_export() -> String {
    let path = super::claude::preload_path();
    if !path.is_file() {
        return String::new();
    }
    posix_preload_export_for(&path)
}

fn posix_preload_export_for(path: &std::path::Path) -> String {
    let p = path.display();
    format!(
        r#"case " ${{BUN_OPTIONS-}} " in
  *" --preload {p} "*) ;;
  *) export BUN_OPTIONS="${{BUN_OPTIONS:+$BUN_OPTIONS }}--preload {p}" ;;
esac
"#
    )
}

fn fish_preload_export() -> String {
    let path = super::claude::preload_path();
    if !path.is_file() {
        return String::new();
    }
    let p = path.display();
    format!(
        r#"if not string match -q "*--preload {p}*" -- "$BUN_OPTIONS"
  set -gx BUN_OPTIONS (string trim -- "$BUN_OPTIONS --preload {p}")
end
"#
    )
}

fn print_posix_init() {
    print!("{}", posix_preload_export());
    print!(
        r#"# lowfat shell init — auto-wrap commands for LLM token savings
# Usage: eval "$(lowfat shell-init zsh)"

if [[ "$CLAUDECODE" == "1" ]] || [[ -n "$CODEX_ENV" ]] || [[ "$LOWFAT_ENABLE" == "1" ]]; then
  for _lf_cmd in $(command lowfat filters --commands 2>/dev/null); do
    eval "$_lf_cmd() {{ command lowfat $_lf_cmd \"\$@\"; }}"
  done
  unset _lf_cmd
fi
"#
    );
}

fn print_fish_init() {
    print!("{}", fish_preload_export());
    print!(
        r#"# lowfat shell init for fish
# Usage: lowfat shell-init fish | source

if test "$CLAUDECODE" = 1; or test -n "$CODEX_ENV"; or test "$LOWFAT_ENABLE" = 1
  for cmd in (command lowfat filters --commands 2>/dev/null)
    eval "function $cmd; command lowfat $cmd \$argv; end"
  end
end
"#
    );
}

#[cfg(test)]
mod tests {
    use super::posix_preload_export_for;
    use std::path::Path;

    #[test]
    fn preload_export_is_idempotent() {
        let snippet = posix_preload_export_for(Path::new("/x/no-agent-summary.js"));
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!("{snippet}{snippet}printf %s \"$BUN_OPTIONS\""))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            "--preload /x/no-agent-summary.js"
        );
    }
}
