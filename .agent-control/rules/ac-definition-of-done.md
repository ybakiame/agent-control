---
id: ac-definition-of-done
version: 2
title: 'Definition of done: fmt -> clippy -D warnings -> test -> e2e, plus UIA for GUI'
topic: quality-gate
content: ''
kind: contract
scope: project
source:
  kind: project
  author: qoder-cn
  created_at: 2026-11-14T13:41:21.741773300Z
  origin: agent-control-mcp
priority: 100
applies_to: {}
tags:
- verification
- gate
- mise
enabled: true
enforcement: advisory
---
Before declaring any code change done, run the full gate via mise and require all green:
1. mise run fmt (cargo fmt --all)
2. mise run check (cargo clippy --workspace --all-targets -- -D warnings) — zero warnings
3. mise run test (cargo test --workspace)
4. mise run e2e (builds the MCP binary, then mcp-e2e.mjs + mcp-http-e2e.mjs + hook-e2e.mjs)
GUI changes additionally require accessibility-tree verification (see ac-gui-verification). Cover new behavior with tests before done: core unit/integration in crates/core/tests/core_tests.rs; protocol/hook e2e in scripts/*.mjs. Type/test success verifies code correctness, not feature correctness — for UI, drive the actual feature in the running app and explicitly say so if visual confirmation is blocked.
