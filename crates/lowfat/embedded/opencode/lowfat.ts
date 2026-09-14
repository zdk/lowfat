import type { Plugin } from "@opencode-ai/plugin"
import { exec as execCb } from "node:child_process"
import { promisify } from "node:util"

// lowfat OpenCode plugin — rewrites commands to run through lowfat for LLM
// token savings. Requires the `lowfat` binary in PATH.
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

export const LowfatOpenCodePlugin: Plugin = async () => {
  try {
    await execAsync("which lowfat")
  } catch {
    console.warn("[lowfat] lowfat binary not found in PATH — plugin disabled")
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
        stdout = (await execAsync(`lowfat rewrite ${shellQuote(command)}`)).stdout
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
