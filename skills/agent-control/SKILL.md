---
name: agent-control
description: Connect to and operate the Agent Control local-first engineering console over MCP and ACP. Use when acting as an agent or harness that must resolve binding rules with provenance, create a constraint session, acknowledge rules, record tamper-evident evidence, author durable project contracts as rules, propose uncertain rules for human review, hand off across harnesses, and verify the evidence chain; or when registering an ACP agent CLI (agents.yaml) to integrate with the console. Triggers include "connect to agent-control", "get applicable rules", "create a session", "acknowledge rules", "record evidence", "save a project rule/contract", "suggest a rule for review", "export/import handoff", "verify evidence", and "integrate my agent with the console". This skill is for USING a running console, not for developing the console's own source.
when_to_use: An agent/harness is about to work on a project governed by Agent Control, or is being wired into the console over MCP/ACP.
---

# Agent Control

## Overview

Agent Control is a local-first console that makes engineering rules **binding and verifiable**
across agents. It answers: which rules apply, where they came from (user preference vs project
contract), whether they conflict with code, and whether the same constraints stay binding after a
handoff to another harness. As a connected agent, you query rules, freeze them into a session,
acknowledge them, and append evidence to a SHA-256 hash chain.

## Using vs developing

This skill guides an agent that **operates against a running console** (a harness doing project
work). It is not the guide for editing the console's Rust source — that lives in the repository's
`AGENTS.md`. If asked to build/modify the console itself, defer to `AGENTS.md` instead.

## Connect

The console exposes an **MCP server** (14 tools + resources + prompts). Pick one transport:

- **Streamable HTTP** — the desktop app auto-starts `http://127.0.0.1:9870/mcp`
  (`mcp-session-id` header). Configure your MCP client to point at that URL. To run the server
  headless: `agent-control-mcp --bind 127.0.0.1:9870`.
- **stdio** — for harnesses that spawn the server process: `agent-control-mcp --stdio`.

Optional `--data-dir <path>` overrides the data root (default `~/.agent-control`). Build the binary
with `mise run build` (→ `target/debug/agent-control-mcp`), or run via `mise run mcp` (stdio).

Verify the connection by listing tools (expect 14) or reading resource `control://overview`.
If your client cannot reach the server, confirm the desktop app is running (HTTP) or that your
harness spawned the stdio binary correctly.

## Core workflow

Follow this order for every unit of work. Tool parameters and JSON examples are in
[references/mcp-tools.md](references/mcp-tools.md).

1. **Resolve rules** — `get_applicable_rules` with `project_path` (absolute), `agent_id`, and any
   `file_paths` / `languages` / `task_tags`. Read `applied_rules` (each carries provenance) and
   `conflicts`. **If conflicts exist, resolve them with the user before proceeding.** Use
   `explain_rule` to justify a specific rule and `check_conflicts` for a conflict-only view.
2. **Create a session** — `create_session` with `task`, `project_path`, `agent_id`. This freezes a
   constraint snapshot. Keep the returned `session_id` and `rules`.
3. **Acknowledge** — `acknowledge_rules` with `session_id`, `agent_id`, and the exact `rule_ids`
   from the snapshot. The server reports missing/unexpected rules; a partial acknowledgment is
   flagged. Required after session start **and after every handoff**.
4. **Record evidence** — `record_evidence` for each constraint-relevant event: tool calls, file
   changes (include `before_sha256` / `after_sha256`), test results. `kind` is one of
   `tool_call`, `file_change`, `test_result`, `constraint_violation`, `session_start`,
   `session_end`, `rule_acknowledgment`, `handoff_out`, `handoff_in`.
5. **Hand off (when switching harness)** — `export_handoff` (`session_id`, `to_agent`, `context`)
   produces a bundle; the next harness calls `import_handoff` (`bundle`, `new_agent`) then
   `acknowledge_rules` again. Use `get_session_context` to resume work another harness started.
6. **Verify** — `verify_evidence` (`session_id`) recomputes the chain and reports tampering/gaps.

Always pass a stable `agent_id` identifying your harness (e.g. `claude-code`, `gemini-cli`).
Project contracts live in `<project>/.agent-control/rules/`; user preferences in
`~/.agent-control/rules/user/`.

**Recording durable project knowledge.** `record_evidence` is session-scoped (a fixed `kind` enum:
tool calls, file changes, test results, violations) — it is *not* a place to store project issues
or findings. When you discover a lasting project contract (e.g. "the upstream `isAsc` API accepts
bool or string; normalize explicitly"), record it as a rule with **`save_rule`**: pass `id`,
`title`, `content`, `agent_id`, `scope: "project"`, `kind: "contract"`, and `project_path`. It writes
a file into the project's `rules/`, shows up in the console, and binds **all future sessions**.
Reusing an `id` updates the rule and bumps its version. For a personal preference use `scope: "user"`
(no `project_path`). Do not hand-edit rule files behind the console's back — use `save_rule` so
provenance and the GUI stay consistent. Onboarded projects ship copy-ready templates in
`<project>/.agent-control/examples/` (not binding until copied into `rules/`).

**Proposing rules for human review.** When a contract is *inferred, uncertain, or needs user
confirmation*, do not write it directly with `save_rule`. Instead call **`suggest_rule`** with
`project_path`, `agent_id`, `id`, `title`, `content`, and a `rationale` explaining the discovery.
The draft is queued as a `pending` suggestion in the console's **Suggestions** view; it does **not**
bind sessions until a user reviews it. The user can edit the draft, **approve** it (it becomes a
binding project rule via the same path as `save_rule`), or **reject** it with a note. Keep working
under the currently applicable rules meanwhile. Use **`list_suggestions`** (`project_path`,
`status`) to check whether your proposals were reviewed; an approved one reports `approved_rule_id`.
Rule of thumb: `save_rule` for contracts you are confident are correct project knowledge;
`suggest_rule` for anything a human should sign off on first.

## ACP harness integration

Instead of calling MCP tools yourself, an agent can be **launched by the console over ACP**: the
console creates the session, injects the binding rules as a system-prompt briefing, wires the
agent-control MCP endpoint into the ACP session, handles file/terminal/permission requests, and
records evidence automatically. Register the agent CLI in `~/.agent-control/agents.yaml`
(`protocol: acp`). See [references/acp-harness.md](references/acp-harness.md) for the YAML format,
permission policies, and how to make your CLI ACP-compatible.

## Resources

- [references/mcp-tools.md](references/mcp-tools.md) — all 14 tools (parameters + returns), the 3
  resources, the `constraint-briefing` prompt, and concrete call examples.
- [references/acp-harness.md](references/acp-harness.md) — `agents.yaml`, harness config, permission
  flow, constraint briefing, automatic evidence, and writing an ACP agent.
- In the source repo: `scripts/mcp-e2e.mjs` and `scripts/mcp-http-e2e.mjs` are runnable lifecycle
  examples; `scripts/mock-acp-agent.mjs` is a minimal ACP agent used by the tests.