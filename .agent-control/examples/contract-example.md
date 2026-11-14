---
# Stable unique id. Reusing an id updates the rule and bumps its version.
id: upstream-api-isasc-type
title: Upstream isAsc parameter has an ambiguous type
# preference | contract | constraint
kind: contract
# Files in rules/ are always project-scope regardless of this value.
scope: project
topic: api-integration
source:
  # user | project | imported | inferred
  kind: project
  author: your-team
  created_at: 2026-01-01T00:00:00Z
# Higher priority wins on conflict (default 100).
priority: 100
tags: [api, upstream]
---
The upstream API's `isAsc` parameter is documented inconsistently: some endpoints accept a
boolean (`true`/`false`), others accept a string (`"true"`/`"false"`).

Before calling any upstream sorting endpoint:
1. Confirm the accepted type for that specific endpoint (check its schema or tests).
2. Normalize the value explicitly; never rely on implicit coercion.
3. Record the confirmed contract here so other agents inherit it.

上游 API 的 `isAsc` 参数类型不确定：部分接口接收 bool，部分接收 string。
调用任何上游排序接口前，先确认该接口接受的类型并显式归一化，不要依赖隐式转换。
