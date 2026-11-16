//! Issue #1 spike: document-rot detection over falsifiable doc claims.
//!
//! Reads claim manifests from `<project>/.agent-control/docs/*.yaml`, evaluates every claim
//! against the live repo, and emits a machine-consumable JSON report (fresh / stale / broken).
//! Prototype only: no MCP tool, no evidence-chain writes, no lib surface changes.
//!
//! Run: `mise exec -- cargo run -p agent-control-core --example doc_drift -- --project . [--out report.json]`
//! Exit codes: 0 = all fresh, 1 = drift detected, 2 = evaluation error.

use std::path::{Path, PathBuf};
use std::process::Command;

use agent_control_core::conflict::{CheckStatus, evaluate_code_checks};
use agent_control_core::model::{CodeCheck, RuleKind, Scope};
use agent_control_core::store::new_rule;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct DocManifest {
    doc: String,
    #[serde(default)]
    claims: Vec<DocClaim>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum ClaimType {
    PathExists,
    ConfigValue,
    CommandRuns,
    RegexInFile,
    Version,
}

#[derive(Debug, Deserialize)]
struct Anchor {
    /// 1-based inclusive line range in the document.
    lines: [usize; 2],
    /// Snippet that must still appear inside the range (detects anchor drift / second-order rot).
    #[serde(default)]
    contains: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DocClaim {
    id: String,
    #[serde(rename = "type")]
    kind: ClaimType,
    #[serde(default)]
    anchor: Option<Anchor>,
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    file: Option<String>,
    #[serde(default)]
    key: Option<String>,
    #[serde(default)]
    expect: Option<String>,
    #[serde(default)]
    pattern: Option<String>,
    #[serde(default)]
    command: Option<String>,
}

#[derive(Debug, Serialize)]
struct ClaimResult {
    id: String,
    #[serde(rename = "type")]
    kind: ClaimType,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor: Option<[usize; 2]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    anchor_ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expected: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    detail: Option<String>,
}

#[derive(Debug, Default, Serialize)]
struct Counts {
    fresh: usize,
    stale: usize,
    broken: usize,
}

#[derive(Debug, Serialize)]
struct DocReport {
    doc: String,
    manifest: String,
    doc_exists: bool,
    rot_score: f64,
    counts: Counts,
    claims: Vec<ClaimResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct Report {
    schema: &'static str,
    project_path: String,
    evaluated_at: String,
    documents: Vec<DocReport>,
    totals: Counts,
    rot_score: f64,
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut project = PathBuf::from(".");
    let mut out: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--project" => {
                i += 1;
                project = PathBuf::from(args.get(i).expect("--project needs a value"));
            }
            "--out" => {
                i += 1;
                out = Some(PathBuf::from(args.get(i).expect("--out needs a value")));
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
        i += 1;
    }
    match run(&project) {
        Ok(report) => {
            let json = serde_json::to_string_pretty(&report).expect("report serializes");
            if let Some(path) = out {
                if let Err(err) = std::fs::write(&path, &json) {
                    eprintln!("failed to write {}: {err}", path.display());
                    std::process::exit(2);
                }
            } else {
                println!("{json}");
            }
            let drift = report.totals.stale + report.totals.broken;
            std::process::exit(if drift > 0 { 1 } else { 0 });
        }
        Err(err) => {
            eprintln!("evaluation error: {err:#}");
            std::process::exit(2);
        }
    }
}

fn run(project: &Path) -> Result<Report> {
    let project = project
        .canonicalize()
        .with_context(|| format!("project path {}", project.display()))?;
    let dir = project.join(".agent-control").join("docs");
    let mut manifests: Vec<PathBuf> = std::fs::read_dir(&dir)
        .with_context(|| format!("manifest dir {}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "yaml" || e == "yml"))
        .collect();
    manifests.sort();
    anyhow::ensure!(
        !manifests.is_empty(),
        "no claim manifests in {}",
        dir.display()
    );

    let mut documents = Vec::new();
    let mut totals = Counts::default();
    for manifest in &manifests {
        let report = eval_manifest(&project, manifest);
        totals.fresh += report.counts.fresh;
        totals.stale += report.counts.stale;
        totals.broken += report.counts.broken;
        documents.push(report);
    }
    let total = totals.fresh + totals.stale + totals.broken;
    let rot_score = if total == 0 {
        0.0
    } else {
        (totals.stale + totals.broken) as f64 / total as f64
    };
    Ok(Report {
        schema: "doc-drift-report/1",
        project_path: project.display().to_string(),
        evaluated_at: chrono::Utc::now().to_rfc3339(),
        documents,
        totals,
        rot_score,
    })
}

fn eval_manifest(project: &Path, manifest: &Path) -> DocReport {
    let manifest_rel = manifest
        .strip_prefix(project)
        .unwrap_or(manifest)
        .display()
        .to_string();
    let raw = match std::fs::read_to_string(manifest) {
        Ok(raw) => raw,
        Err(err) => return error_report(manifest_rel, format!("manifest unreadable: {err}")),
    };
    let parsed: DocManifest = match serde_yaml::from_str(&raw) {
        Ok(parsed) => parsed,
        Err(err) => return error_report(manifest_rel, format!("manifest invalid: {err}")),
    };
    let doc_path = project.join(&parsed.doc);
    let doc_lines = std::fs::read_to_string(&doc_path)
        .ok()
        .map(|text| text.lines().count());
    let mut report = DocReport {
        doc: parsed.doc.clone(),
        manifest: manifest_rel,
        doc_exists: doc_lines.is_some(),
        rot_score: 0.0,
        counts: Counts::default(),
        claims: Vec::new(),
        error: None,
    };
    for claim in &parsed.claims {
        let mut result = if doc_lines.is_none() {
            ClaimResult {
                id: claim.id.clone(),
                kind: claim.kind,
                status: "broken",
                anchor: claim.anchor.as_ref().map(|a| a.lines),
                anchor_ok: None,
                expected: None,
                actual: None,
                detail: Some("document missing".into()),
            }
        } else {
            eval_claim(project, claim)
        };
        if let (Some(anchor), Some(lines)) = (&claim.anchor, doc_lines) {
            result.anchor = Some(anchor.lines);
            result.anchor_ok = Some(anchor_ok(&doc_path, anchor, lines));
        }
        match result.status {
            "fresh" => report.counts.fresh += 1,
            "stale" => report.counts.stale += 1,
            _ => report.counts.broken += 1,
        }
        report.claims.push(result);
    }
    let total = report.claims.len();
    report.rot_score = if total == 0 {
        0.0
    } else {
        (report.counts.stale + report.counts.broken) as f64 / total as f64
    };
    report
}

fn error_report(manifest: String, error: String) -> DocReport {
    DocReport {
        doc: String::new(),
        manifest,
        doc_exists: false,
        rot_score: 1.0,
        counts: Counts::default(),
        claims: Vec::new(),
        error: Some(error),
    }
}

fn anchor_ok(doc_path: &Path, anchor: &Anchor, line_count: usize) -> bool {
    let [start, end] = anchor.lines;
    if start == 0 || start > end || end > line_count {
        return false;
    }
    match &anchor.contains {
        None => true,
        Some(snippet) => std::fs::read_to_string(doc_path)
            .map(|text| {
                text.lines()
                    .skip(start - 1)
                    .take(end - start + 1)
                    .collect::<Vec<_>>()
                    .join("\n")
                    .contains(snippet.as_str())
            })
            .unwrap_or(false),
    }
}

fn eval_claim(project: &Path, claim: &DocClaim) -> ClaimResult {
    let base = ClaimResult {
        id: claim.id.clone(),
        kind: claim.kind,
        status: "broken",
        anchor: None,
        anchor_ok: None,
        expected: None,
        actual: None,
        detail: None,
    };
    match claim.kind {
        ClaimType::PathExists => {
            let Some(path) = &claim.path else {
                return ClaimResult {
                    detail: Some("claim missing `path`".into()),
                    ..base
                };
            };
            let exists = project.join(path).exists();
            ClaimResult {
                status: if exists { "fresh" } else { "stale" },
                expected: Some("exists".into()),
                actual: Some(if exists { "exists" } else { "missing" }.into()),
                ..base
            }
        }
        ClaimType::ConfigValue | ClaimType::Version => {
            let (Some(file), Some(key), Some(expect)) = (&claim.file, &claim.key, &claim.expect)
            else {
                return ClaimResult {
                    detail: Some("claim missing `file`/`key`/`expect`".into()),
                    ..base
                };
            };
            let mut rule = new_rule(
                &format!("doc-claim-{}", claim.id),
                "doc claim",
                "",
                RuleKind::Contract,
                Scope::Project,
                None,
            );
            rule.code_checks = vec![CodeCheck {
                file: file.clone(),
                key: key.clone(),
                expect: expect.clone(),
                description: None,
            }];
            let results = evaluate_code_checks(&rule, project);
            let Some(result) = results.first() else {
                return ClaimResult {
                    detail: Some("check produced no result".into()),
                    ..base
                };
            };
            let status = match result.status {
                CheckStatus::Pass => "fresh",
                CheckStatus::Fail => "stale",
                CheckStatus::Missing | CheckStatus::Unreadable => "broken",
            };
            ClaimResult {
                status,
                expected: Some(expect.clone()),
                actual: result.actual.clone(),
                detail: matches!(
                    result.status,
                    CheckStatus::Missing | CheckStatus::Unreadable
                )
                .then(|| format!("{:?}: {}: {}", result.status, file, key)),
                ..base
            }
        }
        ClaimType::RegexInFile => {
            let (Some(file), Some(pattern)) = (&claim.file, &claim.pattern) else {
                return ClaimResult {
                    detail: Some("claim missing `file`/`pattern`".into()),
                    ..base
                };
            };
            let raw = match std::fs::read_to_string(project.join(file)) {
                Ok(raw) => raw,
                Err(err) => {
                    return ClaimResult {
                        expected: Some(pattern.clone()),
                        detail: Some(format!("{file} unreadable: {err}")),
                        ..base
                    };
                }
            };
            let re = match regex::Regex::new(pattern) {
                Ok(re) => re,
                Err(err) => {
                    return ClaimResult {
                        expected: Some(pattern.clone()),
                        detail: Some(format!("invalid regex: {err}")),
                        ..base
                    };
                }
            };
            let matched = re.is_match(&raw);
            ClaimResult {
                status: if matched { "fresh" } else { "stale" },
                expected: Some(pattern.clone()),
                actual: Some(if matched { "matched" } else { "no match" }.into()),
                ..base
            }
        }
        ClaimType::CommandRuns => {
            let Some(command) = &claim.command else {
                return ClaimResult {
                    detail: Some("claim missing `command`".into()),
                    ..base
                };
            };
            // Manifest commands are committed, reviewed repo content — the same trust model as
            // rule code_checks, not untrusted input.
            let output = if cfg!(windows) {
                Command::new("cmd")
                    .args(["/C", command])
                    .current_dir(project)
                    .output()
            } else {
                Command::new("sh")
                    .args(["-c", command])
                    .current_dir(project)
                    .output()
            };
            match output {
                Ok(output) => {
                    let code = output.status.code();
                    let fresh = code == Some(0);
                    ClaimResult {
                        status: if fresh { "fresh" } else { "stale" },
                        expected: Some("exit 0".into()),
                        actual: Some(match code {
                            Some(code) => format!("exit {code}"),
                            None => "terminated by signal".into(),
                        }),
                        detail: (!fresh)
                            .then(|| String::from_utf8_lossy(&output.stderr).trim().to_string()),
                        ..base
                    }
                }
                Err(err) => ClaimResult {
                    expected: Some("exit 0".into()),
                    detail: Some(format!("spawn failed: {err}")),
                    ..base
                },
            }
        }
    }
}
