use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::model::{Rule, RuleSource, Scope, SourceKind};

pub fn load_rules_from_dir(dir: &Path, scope: Scope) -> Result<Vec<Rule>> {
    let mut rules = Vec::new();
    if !dir.exists() {
        return Ok(rules);
    }
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("reading rules directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "md" | "yaml" | "yml") {
            continue;
        }
        match load_rule_file(&path, scope) {
            Ok(rule) => rules.push(rule),
            Err(err) => {
                tracing::warn!(path = %path.display(), error = %err, "skipping invalid rule file");
            }
        }
    }
    rules.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(rules)
}

pub fn load_rule_file(path: &Path, scope: Scope) -> Result<Rule> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading rule file {}", path.display()))?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let mut rule: Rule = if ext == "md" {
        parse_frontmatter(&raw)
            .with_context(|| format!("parsing frontmatter in {}", path.display()))?
    } else {
        serde_yaml::from_str(&raw).with_context(|| format!("parsing YAML in {}", path.display()))?
    };
    rule.scope = scope;
    if rule.source.file_path.is_none() {
        rule.source.file_path = Some(path.to_path_buf());
    }
    if rule.checksum.is_empty() {
        rule.checksum = rule.compute_checksum();
    }
    Ok(rule)
}

fn parse_frontmatter(raw: &str) -> Result<Rule> {
    let trimmed = raw.trim_start();
    let Some(rest) = trimmed.strip_prefix("---") else {
        bail!("markdown rule must start with '---' YAML frontmatter");
    };
    let Some(end) = rest.find("\n---") else {
        bail!("unterminated YAML frontmatter (missing closing '---')");
    };
    let yaml = &rest[..end];
    let body = rest[end + 4..].trim_matches(['\r', '\n']).to_string();
    let mut rule: Rule = serde_yaml::from_str(yaml).context("invalid frontmatter YAML")?;
    if rule.content.is_empty() {
        rule.content = body;
    }
    Ok(rule)
}

pub fn rule_file_path(dir: &Path, rule: &Rule) -> PathBuf {
    dir.join(format!("{}.md", rule.id))
}

pub fn write_rule_file(dir: &Path, rule: &Rule) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("creating rules directory {}", dir.display()))?;
    let path = rule_file_path(dir, rule);
    let mut meta = rule.clone();
    meta.content = String::new();
    meta.checksum = String::new();
    meta.source.file_path = None;
    let yaml = serde_yaml::to_string(&meta).context("serializing rule metadata")?;
    let file_content = format!("---\n{yaml}---\n{}\n", rule.content.trim_end());
    std::fs::write(&path, file_content)
        .with_context(|| format!("writing rule file {}", path.display()))?;
    Ok(path)
}

pub fn delete_rule_file(dir: &Path, rule_id: &str) -> Result<bool> {
    for ext in ["md", "yaml", "yml"] {
        let path = dir.join(format!("{rule_id}.{ext}"));
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("deleting rule file {}", path.display()))?;
            return Ok(true);
        }
    }
    Ok(false)
}

pub fn new_rule_source(kind: SourceKind, author: Option<String>) -> RuleSource {
    RuleSource {
        kind,
        file_path: None,
        author,
        created_at: Utc::now(),
        origin: None,
    }
}
