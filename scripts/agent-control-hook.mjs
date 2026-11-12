#!/usr/bin/env node
// Agent Control — Claude Code SessionStart hook.
//
// Pushes the project's binding rules into the session context at start, so Claude Code inherits
// the contracts before it calls any MCP tool. Read-only: it calls get_applicable_rules over the
// local MCP HTTP endpoint and injects a concise briefing; it never writes files. If the Agent
// Control daemon is unreachable it exits 0 silently, so a session is never blocked.
//
// Wire it up by adding this to .claude/settings.json (project) or ~/.claude/settings.json (user).
// Copy-ready snippet — adjust the absolute path to this script:
//
//   {
//     "hooks": {
//       "SessionStart": [
//         {
//           "hooks": [
//             {
//               "type": "command",
//               "command": "node \"C:/path/to/agent-control/scripts/agent-control-hook.mjs\""
//             }
//           ]
//         }
//       ]
//     }
//   }
//
// Environment:
//   AGENT_CONTROL_MCP_URL          MCP endpoint (default http://127.0.0.1:9870/mcp)
//   AGENT_CONTROL_AGENT_ID         agent id used for resolution (default claude-code)
//   AGENT_CONTROL_HOOK_TIMEOUT_MS  total time budget in ms (default 4000)
//   AGENT_CONTROL_HOOK_DEBUG       if set, log diagnostics to stderr (never blocks the session)

const URL = process.env.AGENT_CONTROL_MCP_URL ?? "http://127.0.0.1:9870/mcp";
const AGENT_ID = process.env.AGENT_CONTROL_AGENT_ID ?? "claude-code";
const TIMEOUT = Number(process.env.AGENT_CONTROL_HOOK_TIMEOUT_MS ?? 4000);
const DEBUG = !!process.env.AGENT_CONTROL_HOOK_DEBUG;

// Hard deadline: a missing/slow daemon must never hang session start.
const guard = setTimeout(() => process.exit(0), TIMEOUT);

function debug(msg) {
  if (DEBUG) process.stderr.write(`agent-control-hook: ${msg}\n`);
}

async function readStdin() {
  if (process.stdin.isTTY) return "";
  const chunks = [];
  try {
    for await (const chunk of process.stdin) chunks.push(chunk);
  } catch {
    /* ignore */
  }
  return Buffer.concat(chunks).toString("utf8");
}

async function post(body, headers) {
  return fetch(URL, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(TIMEOUT),
  });
}

// The Streamable HTTP transport answers with either JSON or SSE framing; handle both.
async function parseBody(res) {
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const text = await res.text();
  if (text.trim().startsWith("{")) return JSON.parse(text);
  for (const line of text.split("\n")) {
    if (!line.startsWith("data:")) continue;
    const payload = line.slice(5).trim();
    if (!payload) continue;
    try {
      const msg = JSON.parse(payload);
      if (msg.id !== undefined) return msg;
    } catch {
      /* keep scanning */
    }
  }
  return null;
}

async function callMcp(tool, args) {
  const headers = {
    "content-type": "application/json",
    accept: "application/json, text/event-stream",
  };
  const initRes = await post(
    {
      jsonrpc: "2.0",
      id: 1,
      method: "initialize",
      params: {
        protocolVersion: "2024-11-05",
        capabilities: {},
        clientInfo: { name: "agent-control-hook", version: "1.0" },
      },
    },
    headers,
  );
  const sid = initRes.headers.get("mcp-session-id");
  if (sid) headers["mcp-session-id"] = sid;
  await initRes.text();

  const noteRes = await post(
    { jsonrpc: "2.0", method: "notifications/initialized", params: {} },
    headers,
  );
  await noteRes.text();

  const res = await post(
    { jsonrpc: "2.0", id: 2, method: "tools/call", params: { name: tool, arguments: args } },
    headers,
  );
  const msg = await parseBody(res);
  if (msg?.error) throw new Error(msg.error.message ?? "mcp error");
  const r = msg?.result;
  if (r?.isError) throw new Error(r.content?.[0]?.text ?? "tool error");
  return r?.structuredContent ?? JSON.parse(r?.content?.[0]?.text ?? "{}");
}

function renderBriefing(data, cwd) {
  const applied = Array.isArray(data?.applied_rules) ? data.applied_rules : [];
  const conflicts = Array.isArray(data?.conflicts) ? data.conflicts : [];
  if (applied.length === 0 && conflicts.length === 0) return "";

  const lines = [
    "# Agent Control — binding rules for this project",
    "",
    `Resolved from Agent Control for ${cwd}. These contracts are binding for this session: follow them and record evidence of your work. Use the agent-control MCP tools (get_applicable_rules, explain_rule, check_conflicts) for provenance and conflict detail.`,
    "",
  ];
  for (const m of applied) {
    const r = m?.rule;
    if (!r) continue;
    lines.push(`## ${r.title} (${r.id})`);
    const meta = [r.kind, r.scope, `priority ${r.priority}`].filter(Boolean).join(" · ");
    if (meta) lines.push(`_${meta}_`);
    if (typeof r.content === "string" && r.content.trim()) {
      lines.push("", r.content.trim());
    }
    if (Array.isArray(m.warnings) && m.warnings.length) {
      lines.push("");
      for (const w of m.warnings) lines.push(`> Warning: ${w}`);
    }
    lines.push("");
  }
  if (conflicts.length) {
    lines.push("## Conflicts — resolve with the user before proceeding", "");
    for (const c of conflicts) {
      const hint = c.resolution_hint ? ` (hint: ${c.resolution_hint})` : "";
      lines.push(`- ${c.description ?? "conflict"}${hint}`);
    }
    lines.push("");
  }
  return lines.join("\n");
}

async function main() {
  const input = await readStdin();
  let cwd = process.cwd();
  try {
    const parsed = JSON.parse(input);
    if (parsed?.cwd) cwd = parsed.cwd;
  } catch {
    /* no/invalid stdin: fall back to process.cwd() */
  }

  const data = await callMcp("get_applicable_rules", {
    project_path: cwd,
    agent_id: AGENT_ID,
    file_paths: [],
    languages: [],
    task_tags: [],
  });
  const briefing = renderBriefing(data, cwd);
  if (!briefing) return null;
  return JSON.stringify({
    hookSpecificOutput: { hookEventName: "SessionStart", additionalContext: briefing },
  });
}

main()
  .then((output) => {
    clearTimeout(guard);
    if (output) {
      process.stdout.write(output, () => process.exit(0));
    } else {
      process.exit(0);
    }
  })
  .catch((e) => {
    clearTimeout(guard);
    debug(e?.message ?? String(e));
    process.exit(0);
  });