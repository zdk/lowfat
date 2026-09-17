import type { Plugin } from "@opencode-ai/plugin"
import { exec as execCb } from "node:child_process"
import { homedir } from "node:os"
import { join } from "node:path"
import { promisify } from "node:util"

// lowfat OpenCode plugin — rewrites commands to run through lowfat for LLM
// token savings. The `lowfat` binary is resolved once at init (PATH first,
// then ~/.local/bin, ~/.cargo/bin, /usr/local/bin) and invoked by absolute
// path, so a GUI-launched desktop's minimal PATH doesn't disable the plugin.
//
// Thin delegating plugin: all rewrite logic lives in `lowfat rewrite`, the
// single source of truth (crates/lowfat/src/commands/rewrite.rs). To change
// which commands get wrapped, edit lowfat — not this file.

// Node's child_process works under both the Bun terminal runtime and the
// Electron/Node desktop runtime. PluginInput.$ (BunShell) is undefined in the
// latter, so the old `$` template-literal calls crashed there.
const execAsync = promisify(execCb)

// Wrap in single quotes so the command reaches `lowfat rewrite` as one argv
// item, matching the template-literal quoting the Bun `$` version had.
function shellQuote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`
}

// Resolve the lowfat binary to an absolute path, once at init. PATH first
// (covers a normal shell login); then well-known install locations — a
// GUI-launched Electron desktop inherits a minimal PATH that may lack
// ~/.local/bin, so a bare `lowfat` on PATH would fail there even though the
// binary is installed. Returns null only when no candidate is executable.
async function resolveLowfat(): Promise<string | null> {
  try {
    const { stdout } = await execAsync("command -v lowfat")
    const onPath = stdout.trim()
    if (onPath) return onPath
  } catch {
    // not on PATH — fall through to known locations
  }
  const candidates = [
    join(homedir(), ".local", "bin", "lowfat"),
    join(homedir(), ".cargo", "bin", "lowfat"),
    "/usr/local/bin/lowfat",
  ]
  for (const candidate of candidates) {
    try {
      await execAsync(`test -x ${shellQuote(candidate)}`)
      return candidate
    } catch {
      // not here either
    }
  }
  return null
}

export const LowfatOpenCodePlugin: Plugin = async () => {
  const lowfat = await resolveLowfat()
  if (lowfat === null) {
    console.warn(
      "[lowfat] lowfat binary not found (PATH, ~/.local/bin, ~/.cargo/bin, /usr/local/bin) — plugin disabled",
    )
    return {}
  }

  return {
    "tool.execute.before": async (input, output) => {
      const tool = String(input?.tool ?? "").toLowerCase()
      if (tool !== "bash" && tool !== "shell") return

      const args = output?.args
      if (!args || typeof args !== "object") return

      const command = (args as Record<string, unknown>).command
      if (typeof command !== "string" || !command) return

      let stdout: string
      try {
        stdout = (await execAsync(`${shellQuote(lowfat)} rewrite ${shellQuote(command)}`)).stdout
      } catch (err) {
        // nothrow equivalent: a non-zero exit that still printed a rewrite
        // is honored; otherwise stdout is empty and the command passes through.
        stdout = (err as { stdout?: string })?.stdout ?? ""
      }
      const rewritten = stdout.trim()
      if (rewritten && rewritten !== command) {
        ;(args as Record<string, unknown>).command = rewritten
      }
    },
  }
}
