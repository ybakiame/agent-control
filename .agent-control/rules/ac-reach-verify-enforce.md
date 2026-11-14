---
id: ac-reach-verify-enforce
version: 1
title: Reach != verify != enforce; ExportTarget != HookTarget
topic: architecture
content: ''
kind: contract
scope: project
source:
  kind: project
  author: qoder-cn
  created_at: 2026-11-14T13:41:21.753695700Z
  origin: agent-control-mcp
priority: 100
applies_to: {}
tags:
- architecture
- assurance
- hooks
- export
enabled: true
enforcement: advisory
---
Core framework invariant — never overstate assurance:
- Two orthogonal axes: a rule's declared enforcement (advisory | verify | hard) is intent; the session's achieved AssuranceTier (Advisory < Verified < Enforced) is computed from evidence. Declaring hard does NOT enforce.
- Verified is earned by verify_constraints running the snapshot's code_checks against the real repo (post-hoc). Enforced is earned ONLY by the deny-capable PreToolUse gate check_tool_call AND only when a session_id is passed. The shared hook resolves its own session (get-or-create, task "hook") and passes that id, so hook-only deployments DO record Enforced; the harness's stdin session id is never used (it is not an Agent Control UUID).
- Export + SessionStart briefing only make rules REACH an agent (Advisory). Without a registered hook mediating calls, hard rules cap at Advisory/Verified.
- ExportTarget (agents | claude | gemini | cursor) is SEPARATE from HookTarget (nine vendors). The gemini EXPORT target stays — Antigravity reads the GEMINI.md pointer. export_native writes a static pointer block (never copies rule text, never commits git-ignored environment notes); re-export is byte-idempotent.
- Recommended MCP call order: get_applicable_rules -> create_session -> acknowledge_rules -> record_evidence -> verify_constraints -> update_work_state -> export_handoff/import_handoff -> verify_evidence -> set_session_status.
