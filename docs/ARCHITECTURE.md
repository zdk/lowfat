# Architecture

High-level view of how `lowfat` reduces token usage for AI agents via two paths:
command-output filtering and file-content compression.

## Mode 1: Command Output Filtering

Wraps a shell command, pipes its output through a plugin/builtin pipeline.

```
       ┌──────────────────────────────────────────┐
       │  AI Agent                                │
       │  $ lowfat <cmd>                          │
       └────────────────────┬─────────────────────┘
                            │
                            ▼
       ┌──────────────────────────────────────────┐
       │              lowfat CLI                  │
       │     parse args → dispatch command        │
       └────────────────────┬─────────────────────┘
                            │ run <cmd>
                            ▼
   ┌───────────────────────────────────────────────────┐
   │                 lowfat Runner                     │
   │                                                   │
   │   exec cmd  ─▶  resolve pipeline  ─▶  filter      │
   │     (real)        (config+plugin)     (chain)     │
   └──────┬───────────────┬───────────────────┬────────┘
          │               │                   │
          ▼               ▼                   ▼
     ┌────────┐     ┌──────────┐       ┌──────────────┐
     │ Config │     │ Plugins  │       │  Builtins    │
     │ .lowfat│     │ embedded │       │  strip-ansi  │
     │  env   │     │ + ~/.lf  │       │  head/grep…  │
     └────────┘     └──────────┘       └──────────────┘
                            │
                            ▼
                  ┌─────────────────────┐
                  │  filtered output    │ ──▶ Agent
                  │  + SQLite metrics   │
                  └─────────────────────┘
```

## Mode 2: File Content Compression (Post-Read Hook)

Intercepts Claude Code's `Read` tool output and compresses file content
based on file type (code, markdown, HTML, data, lock files).

```
   ┌───────────────────────────────────────────────────┐
   │  Claude Code reads a file                        │
   │  (PostToolUse hook fires)                        │
   └────────────────────┬─────────────────────────────┘
                        │ stdin: JSON {tool_input, tool_response}
                        ▼
   ┌───────────────────────────────────────────────────┐
   │           lowfat post-read                       │
   │                                                   │
   │   parse hook JSON ─▶ detect content type          │
   │                      ─▶ compress (level-aware)    │
   │                      ─▶ emit updatedToolOutput    │
   └──────────────────────────┬────────────────────────┘
                              │
                              ▼
                  ┌──────────────────────┐
                  │  lowfat-compress     │
                  │                      │
                  │  code   → strip docs │
                  │  md     → trim prose │
                  │  html   → minify     │
                  │  data   → summarize  │
                  │  lock   → digest     │
                  └──────────────────────┘
                              │
                              ▼
                  ┌─────────────────────┐
                  │  compressed content │ ──▶ Claude context
                  └─────────────────────┘
```

## Components

- **lowfat CLI** (`crates/lowfat`) — clap entry point, dispatches subcommands
  (`run`, `post-read`, `stats`, `history`).
- **lowfat Runner** (`crates/lowfat-runner`) — executes the real command, loads
  plugins via `HybridRunner`, and walks the pipeline stages.
- **lowfat Compress** (`crates/lowfat-compress`) — content-aware file compression.
  Routes by detected type (code/markdown/HTML/data/lock) and applies
  level-appropriate reduction. Standalone crate with no dependency on lowfat
  internals.
- **Config** (`crates/lowfat-core`) — resolves `.lowfat` TOML + env vars into a
  `RunfConfig` (level, plugin dir, conditional pipelines).
- **Plugins** (`crates/lowfat-plugin`) — manifest + `.lf` DSL files. Bundled
  plugins live in the crate's `embedded/` dir and are baked in via
  `include_str!`; community plugins live at the repo-root `plugins/`; user
  plugins live under `~/.lowfat/plugins/`. A same-named user plugin overrides
  the bundled one.
- **Builtins** — in-process processors (`strip-ansi`, `head`, `grep`,
  `dedup-blank`, `normalize`, …) used as pipeline stages.
- **SQLite metrics** — `$XDG_DATA_HOME/lowfat` (default `~/.local/share/lowfat`,
  override with `$LOWFAT_DATA`) holds `history.db`, tracking token savings and
  invocation history (powers `lowfat stats` and `lowfat history`).
