use std::collections::HashMap;

use chrono::Utc;
use globset::{Glob, GlobSetBuilder};
use uuid::Uuid;

use crate::model::{
    Conflict, ConflictKind, Decision, MatchReason, ProvenanceRecord, QueryContext, Resolution,
    Rule, RuleMatch, Scope,
};

pub struct RuleEngine;

impl RuleEngine {
    pub fn resolve(rules: &[Rule], ctx: &QueryContext) -> Resolution {
        let mut trace: Vec<RuleMatch> = Vec::new();
        let mut candidates: Vec<RuleMatch> = Vec::new();

        for rule in rules {
            if !rule.enabled {
                trace.push(RuleMatch {
                    rule: rule.clone(),
                    decision: Decision::Disabled,
                    reasons: vec![MatchReason::Excluded {
                        reason: "rule is disabled".into(),
                    }],
                    superseded_by: None,
                    warnings: vec![],
                });
                continue;
            }
            match match_reasons(rule, ctx) {
                Some(reasons) => candidates.push(RuleMatch {
                    rule: rule.clone(),
                    decision: Decision::Applied,
                    reasons,
                    superseded_by: None,
                    warnings: vec![],
                }),
                None => trace.push(RuleMatch {
                    rule: rule.clone(),
                    decision: Decision::FilteredOut,
                    reasons: vec![MatchReason::Excluded {
                        reason: exclusion_reason(rule, ctx),
                    }],
                    superseded_by: None,
                    warnings: vec![],
                }),
            }
        }

        let mut groups: HashMap<String, Vec<RuleMatch>> = HashMap::new();
        let mut applied: Vec<RuleMatch> = Vec::new();
        for m in candidates {
            // Environment rules are descriptive machine/project context: always applied, never
            // competing on a topic. Everything else groups by competition key for precedence.
            if m.rule.scope == Scope::Environment {
                applied.push(m);
            } else {
                groups.entry(m.rule.competition_key()).or_default().push(m);
            }
        }

        for (_key, mut group) in groups {
            group.sort_by(|a, b| {
                b.rule
                    .scope
                    .rank()
                    .cmp(&a.rule.scope.rank())
                    .then(b.rule.priority.cmp(&a.rule.priority))
                    .then(b.rule.source.created_at.cmp(&a.rule.source.created_at))
                    .then(a.rule.id.cmp(&b.rule.id))
            });
            let winner = group.first().expect("group is non-empty").clone();
            for loser in group.into_iter().skip(1) {
                let mut reasons = loser.reasons.clone();
                reasons.push(precedence_reason(&winner.rule, &loser.rule));
                trace.push(RuleMatch {
                    rule: loser.rule,
                    decision: Decision::Superseded,
                    reasons,
                    superseded_by: Some(winner.rule.id.clone()),
                    warnings: vec![],
                });
            }
            applied.push(winner);
        }

        applied.sort_by(|a, b| a.rule.id.cmp(&b.rule.id));

        let mut conflicts = detect_explicit_conflicts(&mut applied);
        conflicts.extend(crate::conflict::detect_code_conflicts(&applied, ctx));

        for conflict in &conflicts {
            for m in applied.iter_mut() {
                if conflict.rule_ids.contains(&m.rule.id) {
                    m.warnings.push(conflict.description.clone());
                    if conflict.kind == ConflictKind::RuleVsRule {
                        m.decision = Decision::ConflictFlagged;
                    }
                }
            }
        }

        trace.extend(applied.iter().cloned());
        trace.sort_by(|a, b| a.rule.id.cmp(&b.rule.id));

        Resolution {
            applied,
            trace,
            conflicts,
        }
    }

    pub fn provenance(rules: &[Rule], ctx: &QueryContext) -> (ProvenanceRecord, Resolution) {
        let resolution = Self::resolve(rules, ctx);
        let record = ProvenanceRecord {
            id: Uuid::new_v4(),
            timestamp: Utc::now(),
            session_id: ctx.session_id,
            agent_id: ctx.agent_id.clone(),
            context: ctx.clone(),
            trace: resolution.trace.clone(),
            conflicts: resolution.conflicts.clone(),
        };
        (record, resolution)
    }
}

fn precedence_reason(winner: &Rule, loser: &Rule) -> MatchReason {
    if winner.scope != loser.scope {
        MatchReason::ScopePrecedence {
            winner: winner.scope,
            over: loser.scope,
        }
    } else if winner.priority != loser.priority {
        MatchReason::PriorityWin {
            winner: winner.priority,
            over: loser.priority,
        }
    } else {
        MatchReason::RecencyWin
    }
}

fn match_reasons(rule: &Rule, ctx: &QueryContext) -> Option<Vec<MatchReason>> {
    let mut reasons = Vec::new();
    let a = &rule.applies_to;

    if !a.agents.is_empty() {
        let matched = a
            .agents
            .iter()
            .find(|pattern| glob_matches(pattern, &ctx.agent_id));
        let pattern = matched?;
        reasons.push(MatchReason::AgentMatch {
            pattern: pattern.clone(),
            agent: ctx.agent_id.clone(),
        });
    }

    if !a.paths.is_empty() {
        let path_match = ctx.file_paths.iter().find_map(|path| {
            a.paths
                .iter()
                .find(|pattern| glob_matches(pattern, path))
                .map(|pattern| (pattern.clone(), path.clone()))
        });
        match path_match {
            Some((pattern, path)) => reasons.push(MatchReason::PathMatch { pattern, path }),
            None => {
                if !ctx.file_paths.is_empty() {
                    return None;
                }
            }
        }
    }

    if !a.languages.is_empty() {
        match a.languages.iter().find(|lang| ctx.languages.contains(lang)) {
            Some(lang) => reasons.push(MatchReason::LanguageMatch {
                language: lang.clone(),
            }),
            None => {
                if !ctx.languages.is_empty() {
                    return None;
                }
            }
        }
    }

    if !a.tasks.is_empty() {
        match a.tasks.iter().find(|tag| ctx.task_tags.contains(tag)) {
            Some(tag) => reasons.push(MatchReason::TaskTagMatch { tag: tag.clone() }),
            None => {
                if !ctx.task_tags.is_empty() {
                    return None;
                }
            }
        }
    }

    if reasons.is_empty() {
        reasons.push(MatchReason::Universal);
    }
    Some(reasons)
}

fn exclusion_reason(rule: &Rule, ctx: &QueryContext) -> String {
    let a = &rule.applies_to;
    if !a.agents.is_empty() && !a.agents.iter().any(|p| glob_matches(p, &ctx.agent_id)) {
        return format!("agent '{}' not in {:?}", ctx.agent_id, a.agents);
    }
    if !a.paths.is_empty()
        && !ctx.file_paths.is_empty()
        && !ctx
            .file_paths
            .iter()
            .any(|f| a.paths.iter().any(|p| glob_matches(p, f)))
    {
        return format!("no context path matches {:?}", a.paths);
    }
    if !a.languages.is_empty()
        && !ctx.languages.is_empty()
        && !a.languages.iter().any(|l| ctx.languages.contains(l))
    {
        return format!(
            "languages {:?} not in context {:?}",
            a.languages, ctx.languages
        );
    }
    if !a.tasks.is_empty()
        && !ctx.task_tags.is_empty()
        && !a.tasks.iter().any(|t| ctx.task_tags.contains(t))
    {
        return format!("task tags {:?} not in context {:?}", a.tasks, ctx.task_tags);
    }
    "no matching condition".into()
}

fn detect_explicit_conflicts(applied: &mut [RuleMatch]) -> Vec<Conflict> {
    let ids: Vec<String> = applied.iter().map(|m| m.rule.id.clone()).collect();
    let mut conflicts = Vec::new();
    let mut seen: Vec<(String, String)> = Vec::new();
    for m in applied.iter() {
        for other_id in &m.rule.conflicts_with {
            if !ids.contains(other_id) {
                continue;
            }
            let pair = if m.rule.id < *other_id {
                (m.rule.id.clone(), other_id.clone())
            } else {
                (other_id.clone(), m.rule.id.clone())
            };
            if seen.contains(&pair) {
                continue;
            }
            seen.push(pair.clone());
            conflicts.push(Conflict {
                kind: ConflictKind::RuleVsRule,
                rule_ids: vec![pair.0.clone(), pair.1.clone()],
                description: format!(
                    "rules '{}' and '{}' explicitly declare a conflict",
                    pair.0, pair.1
                ),
                resolution_hint: Some(
                    "disable one rule, narrow their applies_to, or merge them under one topic"
                        .into(),
                ),
            });
        }
    }
    conflicts
}

fn glob_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" || pattern == "**" {
        return true;
    }
    let normalized = value.replace('\\', "/");
    let mut builder = GlobSetBuilder::new();
    let Ok(glob) = Glob::new(pattern) else {
        return pattern == value;
    };
    builder.add(glob);
    let Ok(set) = builder.build() else {
        return pattern == value;
    };
    set.is_match(&normalized) || set.is_match(value)
}

/// Competitive precedence, highest first. `Environment` is listed last because it does not compete:
/// it is always-applied descriptive context, not an authority that can supersede a contract.
pub fn scope_precedence_order() -> [Scope; 4] {
    [
        Scope::Session,
        Scope::Project,
        Scope::User,
        Scope::Environment,
    ]
}
