// lowfat v2 plugin — rewrites bash/shell commands through `lowfat rewrite`
// for LLM token savings.
//
// 2026-09-24: ported to the tool hook API. The old approach wrapped
// event.tools[...].execute from the session "context" event, but since
// opencode 2.0.16 the context event's tools are bare definitions
// ({description, input}) with no execute function, so that silently no-oped.
// `ctx.tool.hook("execute.before")` exists in both 2.0.12 and 2.0.16 and
// receives {tool, input} where assigning to a.input changes what executes.

import { exec as execCb } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { promisify } from "node:util";

const execAsync = promisify(execCb);

function shellQuote(value) {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

async function resolveLowfat() {
  try {
    const { stdout } = await execAsync("command -v lowfat");
    const onPath = stdout.trim();
    if (onPath) return onPath;
  } catch { /* not on PATH */ }
  const candidates = [
    join(homedir(), ".local", "bin", "lowfat"),
    join(homedir(), ".cargo", "bin", "lowfat"),
    "/usr/local/bin/lowfat",
  ];
  for (const candidate of candidates) {
    try {
      await execAsync(`test -x ${shellQuote(candidate)}`);
      return candidate;
    } catch { /* not here */ }
  }
  return null;
}

const lowfatBinary = await resolveLowfat();

if (lowfatBinary === null) {
  console.warn("[lowfat] binary not found — plugin disabled");
}

export default {
  id: "lowfat",
  setup: async (ctx) => {
    if (!lowfatBinary) return;
    if (typeof ctx.tool?.hook !== "function") {
      console.warn("[lowfat] tool.hook API unavailable — plugin disabled");
      return;
    }

    // Intercept shell/bash tool execution: rewrite the command first.
    await ctx.tool.hook("execute.before", async (a) => {
      if (a.tool !== "shell" && a.tool !== "bash") return;
      const command = a.input?.command;
      if (typeof command !== "string" || !command) return;
      if (/\|\s*lowfat\b/.test(command)) return; // already rewritten
      try {
        const { stdout } = await execAsync(
          `${shellQuote(lowfatBinary)} rewrite ${shellQuote(command)}`
        );
        const result = stdout.trim();
        if (result && result !== command) {
          a.input = { ...a.input, command: result };
        }
      } catch (err) {
        const stdout = err?.stdout;
        if (stdout) {
          const result = stdout.trim();
          if (result && result !== command) {
            a.input = { ...a.input, command: result };
          }
        }
      }
    });
  },
};
