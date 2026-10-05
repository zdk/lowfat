# OpenCode plugin

Thin plugin that routes commands through lowfat for token savings. Two embedded
variants — `lowfat opencode install` picks the one matching your OpenCode version.

- **v1.x** (`lowfat.ts`, TypeScript): default-exported function returning event-key
  hooks. Installed to `~/.config/opencode/plugins/lowfat.ts`.
- **v2** (`opencode-v2/index.js`, plain JS, no build step): v2 (2.0.x) requires the
  module to export `{id, setup}`, and its `session.context` event no longer carries a
  per-tool `execute`, so the old shape loads fine but silently does nothing. The v2
  variant rewrites via `ctx.tool.hook("execute.before")` (present in both 2.0.12 and
  2.0.16). Installed to `~/.config/opencode/plugins/lowfat-v2/index.js`.

- Install: `lowfat opencode install` — detects the major version via
  `opencode --version` (undetectable → installs v1.x and prints a note).
  Uninstall removes whichever variant(s) are present.
- Mechanism: on shell/bash command execution, calls `lowfat rewrite <cmd>` and swaps
  in the result; silently passes through on any failure.
- Single source of truth: rewrite logic lives in
  `crates/lowfat/src/commands/rewrite.rs`, not in the plugin files. Both variants are
  embedded in the binary via `include_str!`, so install needs no network or repo.
