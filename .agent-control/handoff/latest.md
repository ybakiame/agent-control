# Agent Control — continuity brief

_Refreshed by `import_handoff`. The session has resumed under a new harness; re-acknowledge the rules and re-run `verify_constraints` to re-establish assurance tiers._

- **Task**: implementation-handoff
- **From harness**: qoder-cn
- **Resumed under**: qoder-cn
- **Session**: `e2c261ce-63a2-486d-9d51-13578ac200c5`
- **Created**: 2026-11-14T16:08:37.119792800+00:00
- **Evidence chain head**: `60417081bb35e6c24da07bf6a1542545ca2d9c706df66b3aa792ae307f4f67e3`
- **Bundle hash**: `baaf0adbd7f4560d1ef795c2bbef31c57375332c5aed4a669320a64a57962c74`

## Goal

Hand off Agent Control implementation to a fresh session. The origin session (qoder-cn) now only evaluates and plans; the new session resumes implementation via import_handoff / .agent-control/handoff/latest.md.

## Work state (resumable)

- **done**: #128 hook session lifecycle: hook resolves its own session (task 'hook') and passes session_id to check_tool_call, so hook-only deployments record Enforced
- **done**: #129 nine-harness adapter framework: 4 ConfigCodecs (NestedJson/FlatJson/NamedBundleJson/Toml); 7 project-scope + 2 user-scope (Kimi/Antigravity, still gating per-project by cwd)
- **done**: #130 optional fail-closed gate (AGENT_CONTROL_HOOK_FAIL_CLOSED), gate-only; start event stays fail-open; 7 new hook-e2e cases
- **done**: #131 docs sync (README EN/zh, AGENTS.md, SKILL.md, mcp-tools.md) + full gate green: fmt, clippy 0 warnings, 104 tests, 3 e2e suites
- **done**: Commit 661bc41 (nine harnesses + fail-closed + docs) pushed to origin/master
- **done**: Persist project contracts + environment notes + work state + handoff into production Agent Control (10 rules applied, 0 conflicts)
- **done**: Production refreshed to latest master (user did it). Verified: detect_hooks reports 9 harnesses + scope, 10 rules applied / 0 conflicts, handoff session e2c261ce intact (handed_off, 8 work items).
- **done**: #132 FIX Qoder CN project config path (.qoder/settings.json shared with international Qoder) + register/revoke dedupe by config path; committed aa2d960 (not pushed)
- **done**: #133 superseded handoff semantics: HandoffRecord.superseded (serde default, no migration); export_handoff marks prior unacknowledged records superseded; sessions-view 已取代/superseded tag (sess.superseded); core test export_handoff_supersedes_prior_unacknowledged; AGENTS.md + mcp-tools.md synced
- **done**: #134 work-state last-updated line: sessions-view Work state shows session.updated_at in local time via sess.work_updated (Last updated: {} / 最近更新：{}); UIA-verified in en + zh on portable dev GUI (:9871)
- **done**: Round-3 gate green: fmt, clippy 0 warnings, 106 tests, 3 e2e suites; UIA verification of both UX features in en+zh; dev processes cleaned, production untouched. Changes uncommitted in working tree.
- **done**: Production re-refreshed by user (binary updated). Eval live-verified on :9870 — detect_hooks: qoder & qoder-cn both resolve .qoder/settings.json (path fix live); controlled export e7816c62 proved supersede-on-export (6 prior unacked -> superseded=true); 10 rules applied / 0 conflicts; goal restored. GUI pixels confirm 已取代 tags on superseded bundles + 已确认 on acknowledged; work-state timestamp proven via updated_at + dev en/zh UIA. aa2d960 committed-not-pushed; #133/#134 UX changes uncommitted (8 files) — commit left to implementation session.
- **todo**: Hooks registration DEFERRED (user decision): the .qoder path fix has LANDED, so registration would now write the correct shared .qoder/settings.json. Reach is already covered by the AGENTS.md managed block + handoff/latest.md, and all authored rules are advisory (no hard/code_checks/tool_guards) so the PreToolUse gate is fail-open and adds no Enforced tier yet. Register hooks (at least SessionStart for push-briefing) only when a hard rule exists or proactive briefing is wanted.
- **todo**: (Parked, needs user authorization) deeper enforcement roadmap: Connect-harness / broader PreToolUse / repo-gate expansion. Do not start without the user.

## Frozen constraints

- `ac-definition-of-done` · Definition of done: fmt -> clippy -D warnings -> test -> e2e, plus UIA for GUI · contract
- `ac-engineering-conventions` · Agent Control engineering conventions (Rust-first, mise, i18n, core purity) · contract
- `ac-github-push-proxy` · GitHub network access: inline proxy, never edit git config · constraint
- `ac-gui-verification` · Verify the GUI via the UIA tree; Windows build/debug gotchas · contract
- `ac-production-instance` · Production Agent Control install — never disturb; isolate dev · constraint
- `ac-reach-verify-enforce` · Reach != verify != enforce; ExportTarget != HookTarget · contract
- `accessibility-tree-gui` · Drive GUIs via accessibility tree, not mouse clicks · constraint
- `concise-zh-replies` · Reply concisely in Simplified Chinese · preference
- `milestone-commits` · Commit only at verified milestones on request · preference
- `windows-raster-tooling` · Use PowerShell System.Drawing for raster/ICO on Windows · constraint

## Assurance tiers

- `ac-definition-of-done`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `ac-engineering-conventions`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `ac-github-push-proxy`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `ac-gui-verification`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `ac-production-instance`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `ac-reach-verify-enforce`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `accessibility-tree-gui`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `concise-zh-replies`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `milestone-commits`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory
- `windows-raster-tooling`: declared **advisory**, achieved **advisory** — no mechanical check; semantic rule stays advisory

