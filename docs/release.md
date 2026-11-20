# Release & versioning

> Part of the Agent Control engineering conventions. Golden rules, toolchain commands and
> the must-know-before-edit list live in [AGENTS.md](../AGENTS.md); this file holds the detail.

## Semver policy

Workspace version lives in the root `Cargo.toml` (`[workspace.package] version`); all three
crates inherit it. While `0.x`: breaking changes and features bump the **minor**, fixes bump
the **patch**; prereleases use `-rc.N`. After `1.0` the standard semver rules apply.

## Changelog

`CHANGELOG.md` follows [Keep a Changelog](https://keepachangelog.com/): one section per
released version with `Added` / `Changed` / `Fixed` / `Removed` groups, newest first.

## Release runbook

1. Gate green: `mise run fmt` → `mise run check` → `mise run test` → `mise run e2e`
   (GUI changes additionally UIA-verified en+zh).
2. Bump `[workspace.package] version` in `Cargo.toml`.
3. Add the `CHANGELOG.md` entry for the new version.
4. `mise run release` → `scripts/package-release.mjs` reads the Cargo version and assembles
   `dist/agent-control-v<ver>-windows-x64.zip` (exes + both READMEs + icon + hook script +
   `portable.marker`; portable by default).
5. Commit, `git tag v<ver>`, push branch + tag (proxy per project constraint).
6. GitHub release notes from the changelog entry.