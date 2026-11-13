---
name: agent-control
description: Connect to and operate the Agent Control local-first engineering console over MCP and ACP. Use when acting as an agent or harness that must resolve binding rules with provenance, create a constraint session, acknowledge rules, record tamper-evident evidence, author durable project contracts as rules, propose uncertain rules for human review, hand off across harnesses, verify the evidence chain, verify constraint assurance tiers, detect stale native exports, export the binding set into native AGENTS.md/CLAUDE.md/.cursor files, or wire a Claude Code SessionStart hook; or when registering an ACP agent CLI (agents.yaml) to integrate with the console. Triggers include "connect to agent-control", "get applicable rules", "create a session", "acknowledge rules", "record evidence", "save a project rule/contract", "suggest a rule for review", "export/import handoff", "export to harnesses", "SessionStart hook", "verify evidence", "verify constraints", "check export drift", and "integrate my agent with the console". This skill is for USING a running console, not for developing the console's own source.
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

The console exposes an **MCP server** (18 tools + resources + prompts). Pick one transport:

- **Streamable HTTP** — the desktop app runs a **background daemon** at `http://127.0.0.1:9870/mcp`
  (`mcp-session-id` header). It reuses an already-running daemon or spawns the sibling
  `agent-control-mcp` binary detached, so the endpoint **stays alive after you close the console**.
  Configure your MCP client to point at that URL. To run it headless yourself:
  `agent-control-mcp --bind 127.0.0.1:9870`, or `agent-control-mcp --daemon` to detach and exit.
  Single-instance: a second launch on a busy address exits cleanly. Stop a daemon with a loopback
  `POST /shutdown` (the console's Stop button does this).
- **stdio** — for harnesses that spawn the server process: `agent-control-mcp --stdio`.

Optional `--data-dir <path>` overrides the data root (default `~/.agent-control`; a `portable.marker`
next to the binary switches to `<exe dir>/data`). Both binaries run with **no console window** on
Windows and log to `<root>/logs/{app,mcp}.log`. Build the binary
with `mise run build` (→ `target/debug/agent-control-mcp`), or run via `mise run mcp` (stdio).

Verify the connection by listing tools (expect 18) or reading resource `control://overview`.
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
5. **Verify constraints** — `verify_constraints` (`session_id`, `agent_id`) runs the snapshot's
   `code_checks` against the real repo and records the **achieved assurance tier** per rule
   (Advisory / Verified / Enforced). This is post-hoc verification, distinct from `verify_evidence`
   (chain integrity). A rule that declares `hard` still only reaches Advisory/Verified without a
   chokepoint that can deny.
6. **Hand off (when switching harness)** — `export_handoff` (`session_id`, `to_agent`, `context`)
   produces a bundle carrying the achieved tiers; the next harness calls `import_handoff` (`bundle`,
   `new_agent`) then `acknowledge_rules` again, and **re-verifies** — a tier drop is warned, not
   blocked. Use `get_session_context` to resume work another harness started.
7. **Verify** — `verify_evidence` (`session_id`) recomputes the chain and reports tampering/gaps.

Always pass a stable `agent_id` identifying your harness (e.g. `claude-code`, `gemini-cli`).
Project contracts live in `<project>/.agent-control/rules/`; user preferences in
`~/.agent-control/rules/user/`.

**Recording durable project knowledge.** `record_evidence` is session-scoped (a fixed `kind` enum:
tool calls, file changes, test results, violations) — it is *not* a place to store project issues
or findings. When you discover a lasting project contract (e.g. "the upstream `isAsc` API accepts
bool or string; normalize explicitly"), record it as a rule with **`save_rule`**: pass `id`,
`title`, `content`, `agent_id`, `scope: "project"`, `kind: "contract"`, and `project_path`. It writes
a file into the project's `rules/`, shows up in the console, and binds **all future sessions**.
Reusing an `id` updates the rule and bumps its version. Optionally pass `enforcement`
(`advisory` default, `verify`, or `hard`) to declare whether the rule is mechanically checkable, and
`code_checks` (`file` / `key` / `expect`) so `verify_constraints` can evaluate it against the repo.
For a personal preference use `scope: "user"`
(no `project_path`). Do not hand-edit rule files behind the console's back — use `save_rule` so
provenance and the GUI stay consistent. Onboarded projects ship copy-ready templates in
`<project>/.agent-control/examples/` (not binding until copied into `rules/`).

**Pausing/resuming a rule.** To temporarily stop a rule from binding without deleting it, call
**`set_rule_enabled`** with `id`, `enabled: false`, `scope`, and `project_path` for project scope.
The rule is excluded from resolution (`Decision::Disabled`) and no longer binds sessions; set
`enabled: true` to restore it. This is the *only* tool that flips enablement — a `save_rule` content
update preserves the existing state, so editing a paused rule never silently re-activates it. The
console's Rules editor exposes the same toggle as an **Enabled** switch. Prefer disabling over
deleting when a contract may apply again later.

**Proposing rules for human review.** When a rule is *inferred, uncertain, or needs user
confirmation*, do not write it directly with `save_rule`. Instead call **`suggest_rule`** with
`agent_id`, `id`, `title`, `content`, and a `rationale` explaining the discovery. Pass
`scope: "project"` + `project_path` for a project contract, or `scope: "user"` (no `project_path`)
for a **personal preference** that follows the user across projects. The draft is queued as a
`pending` suggestion in the console's **Suggestions** view; it does **not** bind sessions until a
user reviews it. The user can edit the draft, **approve** it (it becomes a binding rule via the same
path as `save_rule` — project or user scope as proposed), or **reject** it with a note. Keep working
under the currently applicable rules meanwhile. Use **`list_suggestions`** (`project_path`,
`status`) to check whether your proposals were reviewed; an approved one reports `approved_rule_id`.
Rule of thumb: `save_rule` for rules you are confident are correct; `suggest_rule` for anything a
human should sign off on first.

**Where environment notes go.** Facts about the user's working environment that should hold across
projects — e.g. "I run codex/kimi CLIs inside WSL; the repo lives in WSL, but Windows-native tools
are callable for compile/verify" — belong in a **user-scope `constraint` rule**, not a project
contract or session evidence. Propose it with `suggest_rule { scope: "user", kind: "constraint" }`
(or `save_rule` if confident). User rules always load, are frozen into every session snapshot, and
are injected proactively through the ACP `constraint_briefing` and `get_applicable_rules`, so the
agent is told up front rather than needing a mid-task reminder.

## Reaching agents that don't call MCP (native export + hook)

Most agents read their own entry points (`AGENTS.md`, `CLAUDE.md`, `.cursor/rules`) instead of
calling MCP. Two mechanisms deliver the binding set to them:

- **`export_native`** (`project_path`, optional `include_user: true`) compiles the **enabled**
  contracts into a *managed block* (`<!-- agent-control:begin --> … <!-- agent-control:end -->`)
  inside the project's `AGENTS.md`, `CLAUDE.md` and `.cursor/rules/agent-control.mdc`; with
  `include_user` it also writes personal preferences to `~/.claude/CLAUDE.md`. Everything outside the
  block is preserved (hand-written prose and Cursor frontmatter survive) and re-export is idempotent
  (`created` / `updated` / `unchanged` per target). The console's Projects pane has the same action as
  **Export to harnesses**. Call it after authoring contracts so file-reading agents inherit them.
- **Claude Code SessionStart hook** — `scripts/agent-control-hook.mjs` (dependency-free Node) calls
  `get_applicable_rules` over the MCP HTTP endpoint at session start and injects a briefing into
  context. It is read-only and exits `0` silently if the daemon is down, so it never blocks a session.
  Wire it in `.claude/settings.json` (project) or `~/.claude/settings.json` (user):

  ```json
  {
    "hooks": {
      "SessionStart": [
        { "hooks": [ { "type": "command", "command": "node \"C:/path/to/agent-control/scripts/agent-control-hook.mjs\"" } ] }
      ]
    }
  }
  ```

  Set `AGENT_CONTROL_MCP_URL` if your endpoint differs from `http://127.0.0.1:9870/mcp`.

**Reach ≠ verify ≠ enforce.** Export and the hook make rules *reach* an agent; they do not force
compliance. Two axes stay separate: a rule's declared `enforcement` (`advisory` / `verify` / `hard`)
vs the session's **achieved tier** (`Advisory` < `Verified` < `Enforced`). `verify_constraints` runs
`code_checks` against the real repo to earn `Verified` (post-hoc); `verify_evidence` checks the hash
chain. `Enforced` requires a chokepoint that can deny (the ACP permission round-trip) — declaring
`hard` without one still caps at Advisory/Verified. After editing rules, `check_export_drift` flags
stale native exports (file-vs-store staleness only, not a running session's context) and prompts a
re-export; it never rewrites files.

## ACP harness integration

Instead of calling MCP tools yourself, an agent can be **launched by the console over ACP**: the
console creates the session, injects the binding rules as a system-prompt briefing, wires the
agent-control MCP endpoint into the ACP session, handles file/terminal/permission requests, and
records evidence automatically. Register the agent CLI in `~/.agent-control/agents.yaml`
(`protocol: acp`). See [references/acp-harness.md](references/acp-harness.md) for the YAML format,
permission policies, and how to make your CLI ACP-compatible.

## Resources

- [references/mcp-tools.md](references/mcp-tools.md) — all 18 tools (parameters + returns), the 3
  resources, the `constraint-briefing` prompt, and concrete call examples.
- [references/acp-harness.md](references/acp-harness.md) — `agents.yaml`, harness config, permission
  flow, constraint briefing, automatic evidence, and writing an ACP agent.
- In the source repo: `scripts/mcp-e2e.mjs` and `scripts/mcp-http-e2e.mjs` are runnable lifecycle
  examples; `scripts/mock-acp-agent.mjs` is a minimal ACP agent used by the tests.