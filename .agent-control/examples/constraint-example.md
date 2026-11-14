---
id: no-secrets-in-source
title: Never commit secrets to source control
kind: constraint
scope: project
topic: security
source:
  kind: project
  author: your-team
  created_at: 2026-01-01T00:00:00Z
priority: 200
tags: [security]
---
Hard constraints for every agent working in this repo:

- Do not hardcode credentials, API keys, tokens, or connection strings in source. Read them from
  environment variables or a local, git-ignored config file.
- Run the test suite before declaring a change complete.
- If a task conflicts with this rule, stop and surface the conflict to the user rather than
  silently weakening the rule.

硬性约束：禁止在源码中硬编码任何密钥/令牌；完成改动前必须运行测试；
任务与本规则冲突时，先向用户暴露冲突，而不是悄悄削弱规则。
