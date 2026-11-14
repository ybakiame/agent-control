//! The deny-capable PreToolUse gate. Pure evaluation: given the enabled `Hard` rules and a tool
//! call, decide allow/deny. A hard rule denies when its repo gate (`code_checks`) finds the repo
//! non-compliant OR a `tool_guard` matches the call; otherwise it mediated the call and counts
//! toward the `Enforced` tier. The Store layer records evidence and assurance around this.

use std::path::Path;

use globset::Glob;
use regex::Regex;
use serde_json::Value;

use crate::conflict::{CheckStatus, evaluate_code_checks};
use crate::model::{GateDenial, GateSource, Rule, ToolGateReport, ToolGuard};

/// Evaluate the gate for one tool call against the enabled `Hard` rules. `tool_input` is matched as
/// its JSON serialization so a single regex can cover commands, paths, or any other input field.
pub fn evaluate_gate(
    hard_rules: &[Rule],
    project_path: &Path,
    tool_name: &str,
    tool_input: &Value,
) -> ToolGateReport {
    let input_text = serde_json::to_string(tool_input).unwrap_or_default();
    let mut denials = Vec::new();
    let mut enforced_rules = Vec::new();

    for rule in hard_rules {
        let mut denied = false;

        // Repo gate: a hard rule whose checks fail (or are inconclusive) blocks tools until fixed.
        for check in evaluate_code_checks(rule, project_path) {
            if check.status != CheckStatus::Pass {
                denials.push(GateDenial {
                    rule_id: rule.id.clone(),
                    source: GateSource::RepoGate,
                    reason: format!(
                        "repo gate: {}:{} expected '{}' ({})",
                        check.file,
                        check.key,
                        check.expect,
                        check_status_label(check.status)
                    ),
                });
                denied = true;
            }
        }

        // Tool guards: deny specific calls matching the tool glob + input regex.
        for guard in &rule.tool_guards {
            if guard_matches(guard, tool_name, &input_text) {
                let reason = if guard.reason.is_empty() {
                    format!("tool guard matched on '{tool_name}'")
                } else {
                    guard.reason.clone()
                };
                denials.push(GateDenial {
                    rule_id: rule.id.clone(),
                    source: GateSource::ToolGuard,
                    reason,
                });
                denied = true;
            }
        }

        if !denied {
            enforced_rules.push(rule.id.clone());
        }
    }

    ToolGateReport {
        allowed: denials.is_empty(),
        tool_name: tool_name.to_string(),
        denials,
        enforced_rules,
        hard_rules_checked: hard_rules.len(),
    }
}

fn check_status_label(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "pass",
        CheckStatus::Fail => "fail",
        CheckStatus::Missing => "missing",
        CheckStatus::Unreadable => "unreadable",
    }
}

fn guard_matches(guard: &ToolGuard, tool_name: &str, input_text: &str) -> bool {
    if !guard.tool.is_empty() && !glob_matches(&guard.tool, tool_name) {
        return false;
    }
    if guard.pattern.is_empty() {
        return true;
    }
    // An invalid regex must not silently deny everything; treat it as a non-match.
    Regex::new(&guard.pattern)
        .map(|re| re.is_match(input_text))
        .unwrap_or(false)
}

/// Match a tool name against a glob, allowing `A|B` alternation (each part its own glob).
fn glob_matches(pattern: &str, text: &str) -> bool {
    pattern.split('|').any(|part| {
        let part = part.trim();
        if part.is_empty() {
            return false;
        }
        match Glob::new(part) {
            Ok(glob) => glob.compile_matcher().is_match(text),
            Err(_) => part == text,
        }
    })
}
