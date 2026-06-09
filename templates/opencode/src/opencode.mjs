// opencode.mjs — manages a child `opencode serve` process and provisions
// per-agent config (agent .md files + opencode.json MCP entries) for an
// opencode-compatible wrapper server. Node 20 ESM, built-ins + global fetch only.

import { spawn, execFile } from "node:child_process";
import { mkdir, readFile, writeFile } from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

const execFileP = promisify(execFile);

// opencode only scans custom agents (`.opencode/agent/*.md`) and per-project
// config when the workspace is a git project. Initialise one (idempotent) with
// a single empty commit so opencode detects the project root on boot.
export async function gitInit(cwd) {
  await mkdir(cwd, { recursive: true });
  const run = (args) => execFileP("git", args, { cwd }).catch(() => {});
  await run(["init", "-q"]);
  await run(["config", "user.email", "agent-server@local"]);
  await run(["config", "user.name", "agent-server"]);
  await run(["commit", "-q", "--allow-empty", "-m", "init"]);
}

// Restart the opencode child (stop, wait for the port to free, start again).
// Needed because opencode loads agents + mcp at boot and does NOT hot-reload —
// so after writing new/updated agent config we reboot to pick it up.
export async function restartOpencode(handle, opts) {
  try {
    handle?.stop?.();
  } catch {
    /* ignore */
  }
  await new Promise((r) => setTimeout(r, 600));
  return startOpencode(opts);
}

// Write an opencode provider into <cwd>/opencode.json so opencode routes model
// calls through a LiteLLM gateway (via opencode's native Anthropic adapter,
// which POSTs to {baseURL}/messages). Models are addressed as "<id>/<model>".
// Merges into any existing config (preserves mcp). No-op if baseURL/apiKey unset.
export async function writeProviderConfig(cwd, { id = "litellm", name = "LiteLLM", baseURL, apiKey, models = [] }) {
  if (!baseURL || !apiKey) return;
  const file = path.join(cwd, "opencode.json");
  let obj = {};
  try {
    obj = JSON.parse(await readFile(file, "utf8"));
  } catch {
    obj = {};
  }
  obj.provider = obj.provider || {};
  obj.provider[id] = {
    npm: "@ai-sdk/anthropic",
    name,
    options: { baseURL, apiKey },
    models: Object.fromEntries(models.map((m) => [m, {}])),
  };
  await mkdir(cwd, { recursive: true });
  await writeFile(file, JSON.stringify(obj, null, 2));
}

// Spawns `opencode serve`, returns once health check passes.
// Returns { baseUrl, proc, stop() }
export async function startOpencode({ port = 4096, cwd, env } = {}) {
  const baseUrl = `http://127.0.0.1:${port}`;
  const proc = spawn(
    "opencode",
    ["serve", "--port", String(port), "--hostname", "127.0.0.1"],
    { cwd, env: { ...process.env, ...env }, stdio: "inherit" }
  );

  const stop = () => {
    try {
      proc.kill();
    } catch {
      /* ignore */
    }
  };

  return await new Promise((resolve, reject) => {
    let settled = false;
    const deadline = Date.now() + 30_000;

    proc.on("error", (err) => {
      if (settled) return;
      settled = true;
      reject(new Error(`Failed to spawn opencode: ${err.message}`));
    });

    proc.on("exit", (code) => {
      if (settled) return;
      settled = true;
      reject(new Error(`opencode exited before becoming healthy (code ${code})`));
    });

    const poll = async () => {
      if (settled) return;
      if (Date.now() > deadline) {
        settled = true;
        stop();
        reject(new Error("Timed out waiting for opencode health check"));
        return;
      }
      try {
        const res = await fetch(`${baseUrl}/global/health`);
        if (res.status === 200) {
          settled = true;
          resolve({ baseUrl, proc, stop });
          return;
        }
      } catch {
        /* not up yet */
      }
      setTimeout(poll, 300);
    };

    poll();
  });
}

// Writes <cwd>/.opencode/agent/<agent.id>.md (system prompt + model +
// permissions). MCP servers are written separately by writeMcpConfig so the
// shared opencode.json reflects exactly the union of all agents (no stale
// accumulation across agents).
export async function provisionAgent(cwd, agent) {
  const agentDir = path.join(cwd, ".opencode", "agent");
  await mkdir(agentDir, { recursive: true });

  // Build YAML frontmatter by hand.
  const lines = [];
  lines.push(`description: ${agent?.name || "sandbox agent"}`);
  lines.push("mode: primary");
  if (agent?.model) lines.push(`model: ${agent.model}`);

  const perms = agent?.permissions;
  if (perms && typeof perms === "object" && Object.keys(perms).length) {
    lines.push("permission:");
    for (const [key, value] of Object.entries(perms)) {
      lines.push(`  ${key}: ${value}`);
    }
  }

  const body = agent?.system || "";
  const md = `---\n${lines.join("\n")}\n---\n${body}`;
  const agentFile = path.join(agentDir, `${agent.id}.md`);
  await writeFile(agentFile, md, "utf8");
}

// Rebuild the `mcp` section of <cwd>/opencode.json from the union of all agents'
// mcp_servers. Replacing (not merging) avoids servers from one agent leaking
// into later sessions. Preserves other config (provider, etc.).
export async function writeMcpConfig(cwd, agents) {
  const configPath = path.join(cwd, "opencode.json");
  let obj = {};
  try {
    obj = JSON.parse(await readFile(configPath, "utf8"));
  } catch {
    obj = {};
  }
  const mcp = {};
  for (const agent of agents || []) {
    for (const server of agent?.mcp_servers || []) {
      if (!server || !server.name) continue;
      if (server.command) {
        mcp[server.name] = {
          type: "local",
          command: [server.command, ...(server.args || [])],
          enabled: true,
        };
      } else if (server.url) {
        mcp[server.name] = { type: "remote", url: server.url, enabled: true };
      }
    }
  }
  obj.mcp = mcp;
  await mkdir(cwd, { recursive: true });
  await writeFile(configPath, JSON.stringify(obj, null, 2), "utf8");
}

// Thin proxy helper to the opencode child. Returns the raw fetch Response.
export async function ocFetch(baseUrl, path, init) {
  return fetch(baseUrl + path, init);
}
