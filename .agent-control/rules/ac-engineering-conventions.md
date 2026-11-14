---
id: ac-engineering-conventions
version: 1
title: Agent Control engineering conventions (Rust-first, mise, i18n, core purity)
topic: engineering
content: ''
kind: contract
scope: project
source:
  kind: project
  author: qoder-cn
  created_at: 2026-11-14T13:41:21.739084500Z
  origin: agent-control-mcp
priority: 100
applies_to: {}
tags:
- rust
- mise
- i18n
- conventions
enabled: true
enforcement: advisory
---
Authoritative source: the repo AGENTS.md — read it before editing. Non-negotiables:
- Rust-first: all product logic lives in the three crates core / mcp / app. Node.js appears only in scripts/*.mjs (dependency-free protocol hooks + e2e). Dependency direction: app -> {mcp, core}, mcp -> core, core -> nothing internal.
- Use mise for everything (mise run <task> / mise exec -- cargo ...); never call a globally-installed cargo/rustc. Toolchain pinned (rust 1.99.0), edition 2024, MSRV 1.95, resolver 3.
- core stays free of GUI/transport deps (pure logic + storage). Business logic in core; mcp is a thin protocol adapter; app is presentation.
- Never hardcode user-facing UI strings: every label goes through crates/app/src/i18n.rs STRINGS + self.t(...)/self.tr(...). Keys must be &'static str — runtime-built keys do not compile (use an exhaustive match with static keys).
- The window renders its own title bar (gpui_kit::component::TitleBar); do not reintroduce native OS chrome.
- Add paths to DataLayout, strings to STRINGS, tools to ControlServer — extend the existing owner, do not duplicate. No speculative abstractions or backward-compat shims; comment only a non-obvious why.
