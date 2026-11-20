# AGENTS.md

Engineering conventions for agents (and humans) working on **Agent Control** — a Rust-first,
local-first command console that connects different Agents over **MCP** and enforces contracts
through their native **hooks**, making rule provenance, conflicts, cross-harness continuity and
evidence verifiable.

以开发者为中心、以工程协议为基础的本地优先指挥台。本文件是仓库的工程约定入口，动手前请先读。
细节已拆分到 [docs/](docs/)（见文末索引）；本文件保留铁律、命令与改动前必知。

> Read this file before editing. It defines the commands, the crate boundaries, and the
> non-negotiable conventions (i18n, self-rendered title bar, accessibility-first verification).

---

## Golden rules / 铁律

1. **Rust-first.** All product logic lives in the three Rust crates. Node.js appears only in
   `scripts/*.mjs` as dependency-free protocol hooks and end-to-end checks.
2. **Use mise for everything.** Never call a globally-installed `cargo`/`rustc` directly; the
   toolchain is pinned. Use `mise run <task>` or `mise exec -- cargo ...`.
3. **Never hardcode user-facing UI strings.** Every label goes through the i18n table
   (`crates/app/src/i18n.rs`) and `self.t(...)` / `self.tr(...)`. See
   [conventions.md](docs/conventions.md#i18n--多语言).
4. **The window renders its own title bar.** Do not reintroduce native OS chrome. Use
   `TitleBar` from `gpui_kit::component`. See
   [architecture.md](docs/architecture.md#gui--自渲染标题栏).
5. **Drive the GUI through the accessibility tree, not the mouse.** Use the UIA scripts and
   `InvokePattern`; synthetic clicks are forbidden (DPI artifacts + user interference). See
   [gui-verification.md](docs/gui-verification.md).
6. **Stop the running GUI before rebuilding on Windows**, or the build fails with a file-lock
   `os error 5`. See [gui-verification.md](docs/gui-verification.md).
7. **Keep `core` free of GUI/transport dependencies.** `core` is pure logic + storage; `mcp` and
   `app` depend on it, never the reverse.

---

## Toolchain & commands / 工具链与命令

Pinned in `mise.toml`: **rust 1.99.0**, **cmake latest**. Workspace is **edition 2024**,
**MSRV 1.95** (`Cargo.toml`). `resolver = "3"`.

```sh
mise install          # one-time: install the pinned rust + cmake
mise run build        # cargo build --workspace
mise run check        # cargo clippy --workspace --all-targets -- -D warnings
mise run test         # cargo test --workspace
mise run run          # cargo run -p agent-control-app   (desktop console)
mise run mcp          # cargo run -p agent-control-mcp -- --stdio
mise run fmt          # cargo fmt --all
mise run release      # cargo build --release + package dist/ portable zip
```

Ad-hoc cargo (still through mise): `mise exec -- cargo <args>`.

**Definition of done for any code change:** `mise run fmt` → `mise run check` (zero warnings,
`-D warnings`) → `mise run test` all green. GUI changes additionally require the
accessibility-tree verification in [gui-verification.md](docs/gui-verification.md).
Protocol/hook changes extend `scripts/mcp-e2e.mjs` / `scripts/hook-e2e.mjs` (`mise run e2e`).

---

## Workspace map / 工作区结构

Three crates under `crates/`. Dependency direction: `app → {mcp, core}`, `mcp → core`,
`core → (nothing internal)`.

| Crate | Package | Kind | Responsibility (one line) |
|-------|---------|------|---------------------------|
| core | `agent-control-core` | lib | Rule engine, provenance, conflicts, hybrid storage, evidence hash chain, handoff, registry, native export/drift/revoke, assurance tiers, tool gate, hooks config |
| mcp | `agent-control-mcp` | lib + bin | MCP server (rmcp): 29 tools + resources + prompts; stdio & Streamable HTTP |
| app | `agent-control-app` | bin | GPUI desktop console (gpui-kit): views + per-session Sheet, bilingual UI, self-rendered title bar, project tree, Integration section |

Key files: `core/src/{engine,model,conflict,gate,hooks,export,config,doctor,search}.rs` +
`store/{mod,db,files}.rs`; `mcp/src/{lib,main}.rs`; `app/src/{main,console,i18n,theme,ui}.rs` +
`views/*.rs`. Full public surface, `Store` entry points, the four layers, environment scope,
storage layout and the GUI/view inventory: [architecture.md](docs/architecture.md).

---

## Must know before editing / 改动前必知

- **i18n.** Bilingual EN/zh, runtime-switchable, persisted to `config.toml`. Add a row to
  `STRINGS` in `crates/app/src/i18n.rs` with both languages, then reference it via
  `self.t(key)` / `self.tr(key, &[args])`; placeholders use `ph.*` keys through
  `InputState::set_placeholder`. Never inline a literal in a view. Details:
  [conventions.md](docs/conventions.md#i18n--多语言).
- **Title bar.** Import `TitleBar` from the component root (`gpui_kit::component::TitleBar`;
  the `title_bar` module is private). `main.rs` builds `WindowOptions` from
  `TitleBar::window_options()` + `title_bar_options()`; render
  `TitleBar::new().child(<h_flex …>)` at the top of `Console::render`. Details:
  [architecture.md](docs/architecture.md#gui--自渲染标题栏).
- **GUI verification.** Drive the window by accessible name + `InvokePattern` via
  `scripts/uia-{dump,invoke,toggle,setvalue,shot}.ps1 -Hwnd <hwnd>`; never synthetic mouse
  clicks. Clickable rows need `.role(Role::Button)` + `.aria_label(...)`; offscreen nodes do
  not respond to Invoke (resize/move via the root `TransformPattern` first). The exe reserves
  an 8MB main-thread stack (`crates/app/build.rs`) — keep it. Details + Windows gotchas:
  [gui-verification.md](docs/gui-verification.md).
- **Stop the dev GUI before rebuilding** (`tasklist | grep agent-control-app`,
  `taskkill //PID <pid> //F` in Git Bash). A running exe locks the binary (`os error 5`).
  Kill only dev PIDs identified by path; a production install may coexist.
- **Extend the existing owner.** New paths → `DataLayout` (`core/src/config.rs`); new UI
  strings → `STRINGS`; new MCP tools → `ControlServer` (`crates/mcp/src/lib.rs`, thin adapter
  over `Store`/`RuleEngine`, plus e2e coverage). Business logic belongs in `core`; `mcp` is a
  thin protocol adapter; `app` is presentation. No disk I/O at render time.
- **Reach ≠ verify ≠ enforce.** Export/hooks *deliver* rules; `code_checks` + the evidence
  chain *verify*; only the PreToolUse gate (`check_tool_call`) produces `Enforced`.
  `ExportTarget` (4 files) ≠ `HookTarget` (9 harnesses). Details:
  [architecture.md](docs/architecture.md) and [enforcement-hooks.md](docs/enforcement-hooks.md).
- **Coding conventions.** Edition 2024, rustfmt defaults, clippy `-D warnings` clean;
  `anyhow::Result`/typed errors in libs, no `unwrap()` on fallible I/O; comment only a
  non-obvious *why*; no speculative abstractions or backwards-compat shims. Details:
  [conventions.md](docs/conventions.md#coding-conventions--编码约定).

---

## Docs index / 文档索引

| Doc | Contents |
|-----|----------|
| [docs/architecture.md](docs/architecture.md) | Workspace map & `Store` surface, four layers, environment scope, storage layout, GUI/views/Sheet/Projects tree inventory |
| [docs/mcp-protocol.md](docs/mcp-protocol.md) | Transports, daemon & portable mode, app config, CLI, the 29 tools, recommended call order, e2e |
| [docs/enforcement-hooks.md](docs/enforcement-hooks.md) | Nine harness targets, four config codecs, start/gate events, the gate, fail-open/fail-closed, hook script |
| [docs/gui-verification.md](docs/gui-verification.md) | UIA scripts, typical verification loop, Windows gotchas (file lock, 8MB stack, locked session, offscreen nodes) |
| [docs/conventions.md](docs/conventions.md) | i18n table & helpers, coding conventions, tests |
| [docs/release.md](docs/release.md) | Semver policy, changelog, release runbook & packaging |

User-facing documentation stays in [README.md](README.md) / [README.zh-CN.md](README.zh-CN.md);
they cross-link this file and the docs above.

---

## License

Apache-2.0.

<!-- agent-control:begin -->
<!-- Generated by Agent Control. This block points at .agent-control; manage rules in the console, not here. -->
<!-- agent-control:block-format:3 -->
## Agent Control — project norms & contracts

Start by understanding this project before changing it. Its engineering norms and contracts live in
`.agent-control/rules/` as Markdown files with YAML frontmatter — the source of truth. This block
points at them; it does not copy their text.

1. Read and summarize the **enabled** rules in `.agent-control/rules/` (kinds `contract` and
   `constraint`) so your work follows the project's conventions. Higher `priority` wins; `applies_to`
   (agents / paths / languages / tasks) narrows when a rule binds. Skip rules with `enabled: false`.
2. Personal preferences live in `~/.agent-control/rules/user/` and follow the user across projects.
3. Machine/project-local environment notes (such as agents running inside WSL) live in
   `.agent-control/local/` (git-ignored). They describe THIS machine, are never committed here, and
   reach you through the MCP `get_applicable_rules` briefing or the SessionStart hook — not this file.
4. For precise resolution — provenance, conflicts, what superseded what — call the Agent Control MCP
   `get_applicable_rules` tool. Reading the files directly is the offline fallback.

### Assurance

These norms are delivered (**reach**); `code_checks` and the evidence chain **verify** them. Nothing
here forces compliance — only a hook chokepoint can **enforce**.

### Continuity (optional)

If `.agent-control/handoff/latest.md` exists it carries a previous harness's task, work state and
frozen constraints; read it to inherit continuity without an MCP call. It is a **local, git-ignored
runtime mirror** (like `.agent-control/local/`): it may be absent on a fresh clone, and it is never
committed. The committed contracts in `.agent-control/rules/` remain the shared source of truth;
cross-machine continuity rides MCP (`get_session_context` / `import_handoff`) or the
integrity-verified handoff bundle, not git. Refresh the mirror with `export_handoff`; resume a
handed-off session with `import_handoff`, then `acknowledge_rules` and `verify_constraints`.
<!-- agent-control:end -->
