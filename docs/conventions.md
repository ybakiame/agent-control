# Conventions: i18n & coding

> Part of the Agent Control engineering conventions. Golden rules, toolchain commands and
> the must-know-before-edit list live in [AGENTS.md](../AGENTS.md); this file holds the detail.

## i18n / 多语言

Bilingual **English / 简体中文**, runtime-switchable in Settings, persisted to `config.toml` (`locale`).

- All strings live in the static table `STRINGS: &[(&str, &str, &str)]` in
  `crates/app/src/i18n.rs`, as `(key, English, 简体中文)` rows. Keys are namespaced
  (`nav.*`, `header.*`, `dash.*`, `proj.*`, `rules.*`, `scope.*`, `prov.*`, `decision.*`,
  `reason.*`, `ev.*`, `sess.*`, `sugg.*`, `desc.*`, `kind.*`, `set.*`, `ph.*`, `log.*`, `lang.*`,
  `common.*`, `mcp.*`, `footer.*`).
- Lookup: `i18n::t(locale, key) -> &'static str` (unknown key falls back to the key itself).
  Templates: `i18n::tr(locale, key, &[args]) -> String` replaces each `{}` **in order**.
- In views/console use the entity helpers **`self.t(key)`** and **`self.tr(key, &[args])`**.
  `Console` holds `pub locale: Locale`; `set_locale(locale, window, cx)` saves `config.toml`,
  re-localizes every `InputState` placeholder, and calls `cx.notify()`.
- `Locale` is `En` (default) / `Zh`; `code()` → `"en"`/`"zh"`, `from_code()` parses.

**To add a UI string:** add a row to `STRINGS` with both languages, then reference it via
`self.t`/`self.tr`. Never inline an English or Chinese literal in a view. Placeholders use
`ph.*` keys and are set through `InputState::set_placeholder` (runtime-localized on switch).

---

## Coding conventions / 编码约定

- Edition 2024; format with `mise run fmt` (rustfmt defaults). Keep `clippy -D warnings` clean.
- Library crates return `anyhow::Result` / typed errors (`thiserror`); avoid `unwrap()`/`expect()`
  on fallible I/O in `core`/`mcp`. `expect()` is acceptable only for truly-unreachable
  invariant violations (e.g. window creation in `app/main.rs`).
- Business logic belongs in `core`; `mcp` is a thin protocol adapter; `app` is presentation.
- Add paths to `DataLayout`, strings to `STRINGS`, tools to `ControlServer` — extend the existing
  owner rather than duplicating logic.
- Tests: unit/integration in `crates/core/tests/core_tests.rs`; protocol + hook e2e in
  `scripts/*.mjs`. Cover new behavior before declaring done.
- Don't add speculative abstractions, backward-compat shims, or noise comments. Comment only a
  non-obvious *why* (hidden constraint, invariant, Windows/GPUI workaround).

