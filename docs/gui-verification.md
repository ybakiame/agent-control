# GUI verification (accessibility tree)

> Part of the Agent Control engineering conventions. Golden rules, toolchain commands and
> the must-know-before-edit list live in [AGENTS.md](../AGENTS.md); this file holds the detail.

## GUI verification / 无障碍树驱动

The window exposes a full **UI Automation (AccessKit)** tree. Drive it by element name +
`InvokePattern`, never by synthetic mouse clicks (immune to user interference and DPI scaling;
works even when the Windows session is locked).

PowerShell helpers in `scripts/` (use the window handle `HWND`):

```powershell
scripts/uia-dump.ps1   -Hwnd <hwnd>                 # walk the control-view tree (names/roles/patterns)
scripts/uia-invoke.ps1 -Hwnd <hwnd> -Name "Settings" # InvokePattern: press a button (-Last = last match, e.g. a confirm dialog's OK)
scripts/uia-toggle.ps1 -Hwnd <hwnd> -Name "AGENTS.md" # TogglePattern: flip a Checkbox/Switch (Invoke cannot)
scripts/uia-setvalue.ps1 -Hwnd <hwnd> -Name "C:/path/to/repo" -Value "C:\repo" # ValuePattern: set an Input's text
scripts/uia-shot.ps1   -Hwnd <hwnd> -Out out.png     # capture the window rect (GetWindowRect)
```

`invoke`/`toggle`/`setvalue` match by accessible **Name or AutomationId**; gpui surfaces a control's
label/placeholder as its Name (the internal `.id(...)` is **not** the UIA AutomationId). **Always pass
`-Hwnd`** for a dev window — with no HWND the helpers fall back to the first `agent-control-app`
process, which may be a production install. Run the scripts under the existing `RemoteSigned` policy
(local scripts are trusted); never bypass execution policy.

Typical loop: launch with `mise run run` → find the HWND → `uia-dump` to confirm names →
`uia-invoke` each nav item / the language buttons → `uia-dump` again to assert the localized
names changed → `uia-shot` for pixels. Verify locale persistence by reading `config.toml`.

**Windows gotchas:**

- **Stop the GUI before rebuilding.** A running `agent-control-app.exe` locks the binary;
  `cargo build` then fails with `failed to remove … (os error 5 / 拒绝访问)`. Find and kill it:
  `tasklist | grep agent-control-app` then `taskkill //PID <pid> //F` (Git Bash needs `//`).
- **The exe reserves an 8MB main-thread stack** (`crates/app/build.rs`, Windows
  `/STACK:8388608` link arg). The Projects view's element tree overflows the default 1MB stack
  during gpui layout/paint (silent `thread 'main' has overflowed its stack` exit when opening the
  view); keep the reserve when touching the link args.
- **Locked session ⇒ screenshots show the lock screen / black.** That is not an app bug. The
  UIA tree still returns correct elements, so verify functionally (names, geometry, `config.toml`)
  and note that pixel confirmation is blocked until the session is unlocked.
- **Clickable list rows must be named to be UIA-drivable.** A raw `div().id(...).on_click(...)`
  is pruned from the control view (generic role, no name). Add `.role(Role::Button)` +
  `.aria_label(...)` (see the suggestion/session/rule/provenance rows) so it exposes an Invoke
  node; `Role` comes from `gpui_kit::*`.
- **gpui's AccessKit `Click` is coordinate-based** (it synthesizes a mouse down/up at the node's
  bounds center). Controls scrolled out of view therefore do NOT respond to `InvokePattern`.
  Bring them on-screen first — e.g. resize/move the window via the root `TransformPattern`
  (`Resize`/`Move`) on a tall monitor — then invoke. Prefer this over synthetic mouse clicks.

