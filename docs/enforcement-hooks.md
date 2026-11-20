# Enforcement hooks

> Part of the Agent Control engineering conventions. Golden rules, toolchain commands and
> the must-know-before-edit list live in [AGENTS.md](../AGENTS.md); this file holds the detail.

## Enforcement hooks / 强制 hooks

Layer 2 enforcement rides the harness's **own** hooks — no separate runtime host. `core/src/hooks.rs`
is the config counterpart of `export.rs`'s managed block: it upserts/removes only *our* hook groups in
each harness's own config, preserving every other key and every user-authored hook. A group is "ours"
when a handler command contains `HOOK_MARKER` (`agent-control-hook.mjs`), so no extra marker field is
needed that a harness might reject. Nine vendors differ in config schema, so each `HookTarget` maps to
a `ConfigCodec` that owns the upsert/remove/registered text manipulation:

- `NestedJson` — `{"hooks":{Event:[{matcher?,hooks:[{type,command,timeout}]}]}}` (Claude family, Codex, Qoder CN).
- `FlatJson` — `{"version":1,"hooks":{event:[{command,timeout}]}}` (Cursor; command on the definition).
- `NamedBundleJson` — `{"<bundle>":{Event:[…]}}` (Antigravity).
- `Toml` — `[[hooks]]` tables inside a user TOML config, owned via a marker block edited as **text** so user comments survive (Kimi).

Writes are atomic (temp file + rename) and reversible. `HookScope` records where the config lives:
`Project` (in the repo, shared/committed) for seven vendors, `User` (a user-global file) for Kimi and
Antigravity. A user-level hook still only enforces for the matching project because the gate resolves
rules by the call's `cwd`.

- **Targets** (`HookTarget`, serde lowercase; `HookTarget::ALL` is the nine-element list):
  `Claude` (`.claude/settings.json`), `CodeBuddy` (`.codebuddy/settings.json`), `WorkBuddy`
  (`.workbuddy/settings.json` — a separate product shipping the same engine rebranded), `Qoder`
  (`.qoder/settings.json`), `QoderCn` (`.qoder/settings.json` — the China build **shares the
  international build's project file**; only the user home differs, `~/.qoder-cn` vs `~/.qoder`),
  `Codex` (`.codex/hooks.json`), `Cursor` (`.cursor/hooks.json`), `Antigravity`
  (`~/.gemini/config/hooks.json`, user), `Kimi` (`~/.kimi-code/config.toml`, user). Antigravity replaces
  the former Gemini CLI target. `register_hooks` / `revoke_hooks` / `detect_hooks` are thin `Store`
  adapters over `hooks::{upsert, remove, registered}`. **Verified against the Qoder CN SDK** (brand
  map `{user: ".qoder-cn", project: ".qoder"}`): the project config dir is `.qoder`, not `.qoder-cn`.
  Because the two Qoder builds share one project file, `Store::register_hooks` / `revoke_hooks`
  dedupe targets by config path (first target in request order owns the shared entry; one report
  outcome per file), and `detect_hooks` reports both targets against that same file.
- **Two events written together** (`HookTarget::events() -> HookEvents { start, gate }`, default
  `CLAUDE_EVENTS`): the **start** event (`SessionStart`, Cursor `sessionStart`, Antigravity
  `PreInvocation`; no matcher) injects the binding-rule briefing via `get_applicable_rules` (reach);
  the **gate** event (`PreToolUse`, Cursor `preToolUse`; empty matcher = every tool) runs the gate via
  `check_tool_call` (enforce). Registering is idempotent (`created|updated|unchanged`); revoking prunes
  the events/hooks we emptied and deletes block-only files (`removed|deleted|unchanged`).
- **The gate** (`core/src/gate.rs`, `evaluate_gate`): pure evaluation over the enabled `Hard` rules.
  A hard rule denies when its **repo gate** (`code_checks` against real files) finds the repo
  non-compliant/inconclusive, OR a **tool guard** (tool-name glob + input regex + reason) matches the
  call. Otherwise it mediated the call and counts toward `Enforced`. `Store::check_tool_call` wraps it,
  records a `ToolGate` evidence entry + achieved tiers, and returns `ToolGateReport`. No hard rules →
  allowed (fail-open). This is the **only** path that produces `AssuranceTier::Enforced`.
- **The hook script** (`scripts/agent-control-hook.mjs`, dependency-free Node) serves all nine vendors;
  the registered command bakes in `vendor` + `event` as argv (authoritative even when a harness omits
  `hook_event_name` in stdin). It selects one of three I/O shapes: **claude** (Claude family, Codex,
  Qoder CN, Kimi — exit **2** + stderr deny, briefing via `hookSpecificOutput.additionalContext`),
  **cursor** (always valid JSON: `{permission:"allow"|"deny",user_message,agent_message}`, briefing via
  `{additional_context}`), **antigravity** (`{decision:"ask"|"deny",reason}` — silence/`{}` is denial
  since agy 1.4.x, so allow/fail-open must emit `ask`; stdin nests the call under `toolCall.{name,args}`).
  The **start** event resolves a session (get-or-create, task `hook`) to freeze the snapshot, then
  injects the `get_applicable_rules` briefing + a best-effort `check_export_drift` warning where the
  vendor supports injection (Antigravity's `PreInvocation` has no channel → reach is via the exported
  `GEMINI.md` pointer). When the start stdin reports `source: "compact"` (the vendor re-entering the
  session after context compaction), the hook instead runs the **compact continuity** path: it
  re-injects the briefing PLUS the on-disk `.agent-control/handoff/latest.md` brief and the session's
  live goal/work state (`get_session_context`), reading the brief from disk *before* any daemon call so
  continuity survives an unreachable daemon (and the guard timer). No extra hook group is registered:
  `PreCompact` is observe-only on most vendors (Cursor `preCompact` exposes only `user_message`;
  Antigravity has no compaction event), so the compact session-start is the documented injection
  channel. The **gate** event resolves the same session and calls `check_tool_call` with
  the pending tool name/input **and that `session_id`**, so a mediated hard rule earns `Enforced` and
  the verdict lands in the evidence chain; on `allowed: false` it blocks in the vendor's deny shape.
  **Fail-open by default** (unreachable/slow daemon → the vendor's allow output, never a spurious
  block); set `AGENT_CONTROL_HOOK_FAIL_CLOSED` to opt into **fail-closed**, where a *gate* invocation
  that cannot reach a verdict denies (the start event always stays fail-open). A hard guard timer
  (`AGENT_CONTROL_HOOK_TIMEOUT_MS`, default 4000) emits the verdict-shaped output on timeout.

Tested by `scripts/hook-e2e.mjs` (start briefing + gate deny/allow in each vendor's shape, hook-only
`Enforced` recording, fail-open and opt-in fail-closed against a throwaway repo) and the gate/hooks
codec unit tests in `crates/core` (per-codec upsert/remove/registered text manipulation, no disk writes
to the real user home). Run with `mise run e2e`. When you change the hook flow, update the script + the
e2e together.

