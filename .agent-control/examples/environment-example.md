---
# Environment notes describe THIS user's CURRENT project on THIS machine. They are descriptive
# context, not competing contracts: always applied, never exported into committed AGENTS.md/CLAUDE.md.
# Copy this file into local/ (git-ignored) to activate it; files in local/ are environment-scope.
id: wsl-build-environment
title: Agents run inside WSL; use Windows-native tools to compile/verify
kind: constraint
scope: environment
enforcement: advisory
source:
  kind: project
  author: your-name
  created_at: 2026-01-01T00:00:00Z
tags: [environment, wsl]
---
This project is checked out on a Windows machine, but agents run inside WSL.

- The repo path under WSL maps to a Windows path; Windows-native toolchains (cargo via mise, rustc)
  are callable for compile/verify even though the shell is Linux.
- Do not assume Linux-only paths or tools; confirm which side a command runs on before relying on it.
- This fact is true only on THIS machine. It is git-ignored and delivered via the MCP/hook briefing,
  never committed — another machine or CI may differ. Put portable personal preferences in user scope
  instead, not here.

本机为 Windows，但代理运行在 WSL 内：仓库路径映射到 Windows 路径，编译/验证可调用 Windows 原生工具链
（如经 mise 的 cargo）。此事实仅对本机成立，已被 git 忽略，仅通过 MCP/hook 简报传递，不会提交；
可跨项目跟随的个人偏好请放在用户级，而不是这里。
