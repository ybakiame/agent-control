use std::path::Path;

use crate::model::{Conflict, ConflictKind, QueryContext, Rule, RuleMatch};

pub fn detect_code_conflicts(applied: &[RuleMatch], ctx: &QueryContext) -> Vec<Conflict> {
    let mut conflicts = Vec::new();
    for m in applied {
        for check in &m.rule.code_checks {
            let file_path = ctx.project_path.join(&check.file);
            let actual = match read_config_value(&file_path, &check.key) {
                Ok(Some(value)) => value,
                Ok(None) => continue,
                Err(err) => {
                    tracing::debug!(file = %file_path.display(), error = %err, "code check unreadable");
                    continue;
                }
            };
            if !values_agree(&check.expect, &actual) {
                conflicts.push(Conflict {
                    kind: ConflictKind::RuleVsCode,
                    rule_ids: vec![m.rule.id.clone()],
                    description: format!(
                        "rule '{}' expects {} = '{}' but {} contains '{}'",
                        m.rule.id, check.key, check.expect, check.file, actual
                    ),
                    resolution_hint: Some(format!(
                        "update the rule or change '{}' in {}",
                        check.key, check.file
                    )),
                });
            }
        }
    }
    conflicts
}

/// Outcome of evaluating a single `code_check` against the repo. Unlike conflict detection (which
/// silently skips missing/unreadable files), verification distinguishes them so a missing file can
/// never masquerade as a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Pass,
    Fail,
    Missing,
    Unreadable,
}

/// The result of one `code_check`: what was expected, what was found (if anything), and the verdict.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CheckResult {
    pub file: String,
    pub key: String,
    pub expect: String,
    pub actual: Option<String>,
    pub status: CheckStatus,
}

/// Evaluate every `code_check` on a rule against real repo files. Used by session verification to
/// compute the achieved assurance tier: a rule reaches `Verified` only when all checks `Pass`.
pub fn evaluate_code_checks(rule: &Rule, project_path: &Path) -> Vec<CheckResult> {
    let mut results = Vec::new();
    for check in &rule.code_checks {
        let file_path = project_path.join(&check.file);
        let (actual, status) = if !file_path.exists() {
            (None, CheckStatus::Missing)
        } else {
            match read_config_value(&file_path, &check.key) {
                Ok(Some(value)) => {
                    let status = if values_agree(&check.expect, &value) {
                        CheckStatus::Pass
                    } else {
                        CheckStatus::Fail
                    };
                    (Some(value), status)
                }
                Ok(None) => (None, CheckStatus::Missing),
                Err(_) => (None, CheckStatus::Unreadable),
            }
        };
        results.push(CheckResult {
            file: check.file.clone(),
            key: check.key.clone(),
            expect: check.expect.clone(),
            actual,
            status,
        });
    }
    results
}

fn values_agree(expected: &str, actual: &str) -> bool {
    let e = expected.trim().trim_matches('"').to_lowercase();
    let a = actual.trim().trim_matches('"').to_lowercase();
    e == a
}

fn read_config_value(path: &Path, key: &str) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(path)?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    match ext {
        "json" => {
            let value: serde_json::Value = serde_json::from_str(&raw)?;
            Ok(json_pointer(&value, key).map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            }))
        }
        "toml" => {
            let value: toml::Value = toml::from_str(&raw)?;
            let mut current = &value;
            for part in key.split('.') {
                current = match current.get(part) {
                    Some(next) => next,
                    None => return Ok(None),
                };
            }
            Ok(Some(match current {
                toml::Value::String(s) => s.clone(),
                other => other.to_string(),
            }))
        }
        "yaml" | "yml" => {
            let value: serde_yaml::Value = serde_yaml::from_str(&raw)?;
            Ok(yaml_path(&value, key).map(yaml_scalar_to_string))
        }
        _ => {
            if file_name == ".editorconfig" || ext == "ini" || ext == "cfg" {
                Ok(ini_value(&raw, key))
            } else {
                Ok(key_equals_value(&raw, key))
            }
        }
    }
}

fn json_pointer<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn yaml_path<'a>(value: &'a serde_yaml::Value, key: &str) -> Option<&'a serde_yaml::Value> {
    let mut current = value;
    for part in key.split('.') {
        current = current.get(part)?;
    }
    Some(current)
}

fn yaml_scalar_to_string(value: &serde_yaml::Value) -> String {
    match value {
        serde_yaml::Value::String(s) => s.clone(),
        serde_yaml::Value::Bool(b) => b.to_string(),
        serde_yaml::Value::Number(n) => n.to_string(),
        other => format!("{other:?}"),
    }
}

fn ini_value(raw: &str, key: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if line.starts_with('#') || line.starts_with(';') || line.starts_with('[') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=')
            && k.trim().eq_ignore_ascii_case(key)
        {
            return Some(v.trim().to_string());
        }
    }
    None
}

fn key_equals_value(raw: &str, key: &str) -> Option<String> {
    for line in raw.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(v) = rest.strip_prefix('=') {
                return Some(v.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}
