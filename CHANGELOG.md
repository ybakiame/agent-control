# Changelog

All notable changes to Agent Control are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow the semver policy in
[docs/release.md](docs/release.md) (while `0.x`: breaking/feat → minor, fix → patch).

## [0.2.0-rc.1] - 2026-11-20

First release candidate of the 0.2 line. `0.1.0` was the unreleased development baseline.

### Added

- **Enforcement hooks for nine harnesses** (Claude Code, CodeBuddy, WorkBuddy, Qoder, Qoder CN,
  Codex, Cursor, Antigravity, Kimi Code): each vendor's native config schema via four codecs,
  a start event that injects the binding-rule briefing and a PreToolUse gate that denies calls
  violating hard rules (`check_tool_call`, the only path to `Enforced`). Opt-in fail-closed via
  `AGENT_CONTROL_HOOK_FAIL_CLOSED`; fail-open by default.
- **P1 full-text search** (`search` MCP tool + Dashboard card): FTS5 over rules, sessions and
  evidence with hybrid CJK bigram segmentation so two-character Chinese queries match.
- **P2 compact continuity**: a `SessionStart` with `source: "compact"` re-injects the briefing plus
  the on-disk handoff brief and the session's live goal/work state, surviving an unreachable daemon.
- **P3 one-command onboarding** (`setup` MCP tool + Projects **Setup** button): default export plus
  all nine hook targets in one idempotent call.
- **P4 read-only diagnostics** (`doctor` MCP tool, `--doctor` CLI flag, Settings card): hooks across
  nine harnesses, export drift, every session's chain integrity, conflicts and a daemon probe.
- **Environment scope** rules: git-ignored `.agent-control/local/` machine/project notes,
  briefing-only delivery, non-competitive resolution, Work Environment nav.
- **Rule suggestions** with scope-aware human approval (project / environment / user).
- **Resumable work state** (`update_work_state`: goal + done/doing/todo) carried through handoff
  bundles and `.agent-control/handoff/latest.md`.
- **UX1** session list grouped by project with kind tags (work/continuity/telemetry), derived
  ownership chains, predecessor links and superseded-handoff disclosure.
- **UX2** per-session Sheet consolidating overview, handoffs, provenance and the evidence chain.
- **UX3** day-grouped evidence timeline with per-kind icons, ownership-flip markers, payload
  expansion and a telemetry filter.
- **UX4** navigation regrouped into Workspace / Contracts / System.
- **UX5** per-mutation acting-identity evidence attribution (work state, status, ownership, rename).
- **UX6** automatic handoff export (bundle + brief) when a session closes; manual create panel removed.
- **UX7** human-readable session titles with audited rename (task stays the machine key).
- **UX8** compact full-width session cards with kind filters; the Sheet carries all detail/actions.
- **UX9** Projects **Integration** section: AGENTS.md pointer switch, opt-in inline contract snapshot
  (ruleset-hash drift on rule edits, default off) and a nine-harness hooks switch with read-only
  status disclosure; MCP `export_native` gains `inline`.
- **DOC1** documentation split: `AGENTS.md` slimmed to a 134-line entry point, detail moved into
  `docs/{architecture,mcp-protocol,enforcement-hooks,gui-verification,conventions,release}.md`.
- `CHANGELOG.md` and the release runbook in `docs/release.md`; packaging and the app footer read the
  workspace Cargo version.

### Changed

- Native export writes a static v2 pointer block (byte-idempotent); drift now means missing/older
  format **or** an outdated inline snapshot, never a rule edit in pointer mode.
- Sessions view replaced the list/detail split with cards + Sheet; completed sessions hidden by
  default behind a Show completed switch.
- Handoff semantics: only the latest unacknowledged record is actionable; older ones read superseded.

### Fixed

- Qoder CN project config dir is `.qoder` (shared with the international build); register/revoke
  dedupe targets by config path so the shared file is written once.
- `create_session` get-or-create reuse stops duplicate sessions per harness conversation; a closed
  source session links as `predecessor` instead of reading as a duplicate.
- Content updates via `save_rule`/`upsert_rule` preserve a rule's enabled state; `set_rule_enabled`
  is the only enablement write path.