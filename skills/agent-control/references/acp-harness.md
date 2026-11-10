# ACP harness integration

Over ACP (agent-client-protocol v1), the **console launches your agent CLI** and drives it. This
is the alternative to calling MCP tools yourself: the console handles the protocol lifecycle and
evidence recording, while your agent focuses on the task. Use this when you want the console to
supervise a harness end-to-end (sessions view, permission prompts, automatic evidence).

## Register the agent: `agents.yaml`

Located at `~/.agent-control/agents.yaml` — a YAML list of registrations. If absent, the console
seeds defaults (Claude Code, Gemini CLI). Fields per entry:

- `id` (string) — stable harness id, used as `agent_id` in sessions/evidence.
- `name` (string) — display name.
- `command` (string) — executable that speaks ACP on stdio.
- `args` (string[], default `[]`) — CLI arguments.
- `env` (map string→string, default `{}`) — extra environment variables.
- `protocol` (`acp` | `mcp`) — use `acp` for launched harnesses.

```yaml
- id: claude-code
  name: Claude Code
  command: claude-code-acp
  args: []
  env: {}
  protocol: acp
- id: gemini-cli
  name: Gemini CLI
  command: gemini
  args:
    - --experimental-acp
  env: {}
  protocol: acp
```

The executable must already be on `PATH` (or use an absolute `command`). The console spawns it
with `args`/`env` and speaks ACP v1 over its stdio.

## What the console does automatically

When a harness session starts (`run_harness`), the console:

1. **Creates a control session** for the project + task, freezing the applicable rules into a
   constraint snapshot (`snapshot_hash`).
2. **Builds a constraint briefing** from the snapshot rules (title, scope, kind, priority,
   content) and injects it as the ACP **system prompt**. If the agent does not advertise
   `system_prompt` capability, the briefing is prepended to the **first user prompt** instead.
3. **Wires the agent-control MCP endpoint** into the ACP `session/new` request (`mcp_servers`),
   so the agent can call `record_evidence` etc. The endpoint is either:
   - `Http { url }` — e.g. `http://127.0.0.1:9870/mcp`, or
   - `Stdio { command, args }` — a spawned `agent-control-mcp --stdio`.
4. **Serves the agent's client requests**: read/write text files (scoped to the project root),
   create/manage terminals. A file **write** automatically records `file_change` evidence.
5. **Records `tool_call` evidence** from the agent's `ToolCall` / `ToolCallUpdate` stream
   notifications, and forwards agent messages, plans, and progress as events.
6. **Handles permission requests** according to the policy (below).

Because the console records evidence and freezes constraints for you, a launched harness does not
need to manually run the full MCP workflow — though the MCP endpoint remains available to it.

## Permission flow

`permission_policy` controls how the console answers the agent's `session/request_permission`:

- `ask` (default) — surfaces a `PermissionRequest` event to the console UI; a human selects an
  option. Decisions: `allow_once`, `allow_always`, `reject_once`, `reject_always`, `cancel`.
- `auto_approve` — answers `allow_once` automatically.
- `auto_reject` — answers `reject_once` automatically.

The console maps the decision to a matching option `kind` from the agent's offered options; if no
matching option exists it cancels. Use `ask` for interactive supervision; `auto_approve` only for
trusted, sandboxed runs.

## Make your CLI ACP-compatible

To integrate a new agent:

1. Implement the **agent side of ACP v1** over stdio: respond to `initialize`, `session/new`
   (accept `mcpServers` and optional `systemPrompt`), `session/prompt`, and stream
   `session/update` notifications (`agent_message_chunk`, `tool_call`, `tool_call_update`,
   `plan`). Request `session/request_permission` for guarded actions; use the client's
   `fs/read_text_file`, `fs/write_text_file`, and terminal requests for side effects.
2. Advertise `system_prompt` session capability if you can accept a system prompt; otherwise the
   console prepends the briefing to the first prompt.
3. Register the CLI in `agents.yaml` with `protocol: acp`.
4. Connect the provided agent-control MCP server (passed via `mcp_servers`) to record evidence
   and query rules when needed.

`scripts/mock-acp-agent.mjs` in the source repo is a minimal, dependency-free ACP agent used by
`crates/acp/tests/harness.rs` — it exercises a permission round-trip and file-change evidence and
is the reference for the expected message shapes.

## Verify

Run `mise run test` in the source repo to exercise the ACP flow against the mock agent, or launch
a registered harness from the console's **Sessions & Handoff** view and watch the constraint
briefing, permission prompts, and evidence stream.