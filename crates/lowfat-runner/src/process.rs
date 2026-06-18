use anyhow::{Context, Result};
use lowfat_plugin::plugin::{FilterInput, FilterOutput, FilterPlugin, PluginInfo};
use lowfat_plugin::security;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

/// Runs a shell plugin as an external process via stdin/stdout.
pub struct ProcessFilter {
    pub info: PluginInfo,
    pub entry: PathBuf,
    pub base_dir: PathBuf,
}

impl FilterPlugin for ProcessFilter {
    fn info(&self) -> PluginInfo {
        self.info.clone()
    }

    fn filter(&self, input: &FilterInput) -> Result<FilterOutput> {
        let entry = self.entry.to_string_lossy().to_string();
        let safe_env = security::sanitized_env();

        let mut child = Command::new("sh")
            .arg(&entry)
            .current_dir(&self.base_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_clear()
            .envs(safe_env)
            .env("LOWFAT_LEVEL", input.level.to_string())
            .env("LOWFAT_COMMAND", &input.command)
            .env("LOWFAT_SUBCOMMAND", &input.subcommand)
            .env("LOWFAT_ARGS", input.args.join(" "))
            .env("LOWFAT_EXIT_CODE", input.exit_code.to_string())
            .spawn()
            .with_context(|| format!("failed to spawn plugin: sh {entry}"))?;

        // Threaded write: a filter may exit early or fill its stdout pipe
        // before stdin drains — a blocking write EPIPEs or deadlocks here
        // (issue #9; same pattern as lowfat-core's run_filter_child).
        let writer = child.stdin.take().map(|mut stdin| {
            let data = input.raw.clone();
            std::thread::spawn(move || match stdin.write_all(data.as_bytes()) {
                Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
                r => r,
            })
        });

        let output = child.wait_with_output()?;
        if let Some(w) = writer {
            w.join()
                .map_err(|_| anyhow::anyhow!("plugin stdin writer panicked"))?
                .with_context(|| format!("writing to plugin stdin: {entry}"))?;
        }
        let text = String::from_utf8_lossy(&output.stdout).to_string();

        Ok(FilterOutput {
            passthrough: text.is_empty(),
            text,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lowfat_core::level::Level;
    use std::io::Write;

    fn make_input(raw: &str) -> FilterInput {
        FilterInput {
            raw: raw.to_string(),
            command: "test".to_string(),
            subcommand: "sub".to_string(),
            args: vec!["arg1".to_string()],
            level: Level::Full,
            head_limit: 40,
            exit_code: 0,
        }
    }

    fn make_filter(entry: &str, code: &str) -> ProcessFilter {
        let dir =
            std::env::temp_dir().join(format!("lowfat-test-{}-{}", entry, std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(entry);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(code.as_bytes()).unwrap();

        ProcessFilter {
            info: PluginInfo {
                name: "test-plugin".into(),
                version: "0.1.0".into(),
                commands: vec!["test".into()],
                subcommands: vec![],
            },
            entry: path,
            base_dir: dir,
        }
    }

    #[test]
    fn shell_filter() {
        let filter = make_filter("filter.sh", "#!/bin/sh\ngrep -v '^warning:'");
        let input = make_input("ok line\nwarning: skip\nanother line");
        let result = filter.filter(&input).unwrap();
        assert_eq!(result.text.trim(), "ok line\nanother line");
        assert!(!result.passthrough);
    }

    #[test]
    fn shell_env_vars() {
        let code = "#!/bin/sh\necho \"level=$LOWFAT_LEVEL\"\necho \"cmd=$LOWFAT_COMMAND\"\necho \"sub=$LOWFAT_SUBCOMMAND\"\necho \"args=$LOWFAT_ARGS\"\necho \"exit=$LOWFAT_EXIT_CODE\"";
        let filter = make_filter("env.sh", code);
        let input = make_input("ignored");
        let result = filter.filter(&input).unwrap();
        assert!(result.text.contains("level=full"));
        assert!(result.text.contains("cmd=test"));
        assert!(result.text.contains("sub=sub"));
        assert!(result.text.contains("args=arg1"));
        assert!(result.text.contains("exit=0"));
    }

    #[test]
    fn empty_output_passthrough() {
        let filter = make_filter("empty.sh", "#!/bin/sh\n# output nothing");
        let input = make_input("some input");
        let result = filter.filter(&input).unwrap();
        assert!(result.passthrough);
        assert!(result.text.is_empty());
    }
}
