---
id: ac-gui-verification
version: 2
title: Verify the GUI via the UIA tree; Windows build/debug gotchas
topic: gui-verification
content: ''
kind: contract
scope: project
source:
  kind: project
  author: qoder-cn
  created_at: 2026-11-14T13:41:21.748144300Z
  origin: agent-control-mcp
priority: 100
applies_to: {}
tags:
- gui
- uia
- windows
- debug
enabled: true
enforcement: advisory
---
Drive the GUI through the accessibility tree, never synthetic mouse clicks (DPI artifacts + user interference; works even when the session is locked). PowerShell helpers in scripts/ — ALWAYS pass -Hwnd for a dev window so you never hit the production install:
- uia-dump.ps1 -Hwnd <hwnd> (walk names/roles/patterns; dump before invoking, a11y is lazy)
- uia-invoke.ps1 -Hwnd <hwnd> -Name "<label>" (InvokePattern; -Last for a confirm dialog OK)
- uia-toggle.ps1 (Switch/Checkbox = TogglePattern; Invoke cannot flip it)
- uia-setvalue.ps1 (Input text via ValuePattern; an Input's UIA Name is its placeholder)
- uia-shot.ps1 -Hwnd <hwnd> -Out out.png
Clickable list rows need Role::Button + aria_label to be UIA-drivable. gpui AccessKit Click is coordinate-based, so controls scrolled out of view do NOT respond — bring them on-screen first (root TransformPattern Resize/Move).
Windows gotchas: STOP the running dev GUI before rebuilding or cargo fails with a file-lock os error 5; find/kill via tasklist | grep agent-control-app then taskkill //PID <pid> //F (Git Bash needs //). A locked Windows session blocks screenshots (lock screen/black) but the UIA tree still works — verify functionally and note pixels are blocked. Run scripts under the existing RemoteSigned policy; never bypass execution policy.
