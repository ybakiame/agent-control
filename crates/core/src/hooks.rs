//! Register the Agent Control hook into each harness's `settings.json` so the harness itself drives
//! the SessionStart briefing (reach) and the PreToolUse gate (enforce).
//!
//! This is the JSON counterpart of [`crate::export`]'s managed block: we upsert/remove only *our*
//! hook groups and preserve every other key and every user-authored hook. A group is "ours" when one
//! of its commands references [`HOOK_MARKER`] (the hook script basename), so no extra marker field is
//! needed that a harness might reject. Registering is idempotent; revoking prunes the events/hooks we
//! emptied and leaves the rest of the file untouched.

use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde_json::{Map, Value, json};

use crate::config::DataLayout;
use crate::export::TargetOutcome;

/// Substring that identifies a hook group as managed by Agent Control: any group whose command
/// invokes our hook script is ours. Everything else belongs to the user and is preserved.
pub const HOOK_MARKER: &str = "agent-control-hook.mjs";

/// The hook events we manage. `SessionStart` injects the binding-rule briefing (reach); `PreToolUse`
/// runs the deny-capable gate (enforce). Both are written together so a registered harness both
/// inherits the contracts and mediates tool calls.
pub const HOOK_EVENTS: [&str; 2] = ["SessionStart", "PreToolUse"];

/// A harness whose `settings.json` we can register the hook into. CodeBuddy and WorkBuddy are
/// separate products (WorkBuddy ships the same engine rebranded) with separate `.codebuddy/` and
/// `.workbuddy/` project settings, so they are separate targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookTarget {
    Claude,
    CodeBuddy,
    WorkBuddy,
    Qoder,
}

impl HookTarget {
    pub const ALL: [HookTarget; 4] = [
        HookTarget::Claude,
        HookTarget::CodeBuddy,
        HookTarget::WorkBuddy,
        HookTarget::Qoder,
    ];

    /// Parse a user/MCP-supplied target name, case-insensitive.
    pub fn parse(s: &str) -> Option<HookTarget> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "claude code" => Some(HookTarget::Claude),
            "codebuddy" => Some(HookTarget::CodeBuddy),
            "workbuddy" => Some(HookTarget::WorkBuddy),
            "qoder" => Some(HookTarget::Qoder),
            _ => None,
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            HookTarget::Claude => "claude",
            HookTarget::CodeBuddy => "codebuddy",
            HookTarget::WorkBuddy => "workbuddy",
            HookTarget::Qoder => "qoder",
        }
    }

    /// Human-readable harness name for the console / reports.
    pub fn label(&self) -> &'static str {
        match self {
            HookTarget::Claude => "Claude Code",
            HookTarget::CodeBuddy => "CodeBuddy",
            HookTarget::WorkBuddy => "WorkBuddy",
            HookTarget::Qoder => "Qoder",
        }
    }

    /// Project-level `settings.json` the hook is registered into (shared, committed with the repo).
    pub fn settings_path(&self, project_path: &Path) -> PathBuf {
        match self {
            HookTarget::Claude => DataLayout::project_claude_settings(project_path),
            HookTarget::CodeBuddy => DataLayout::project_codebuddy_settings(project_path),
            HookTarget::WorkBuddy => DataLayout::project_workbuddy_settings(project_path),
            HookTarget::Qoder => DataLayout::project_qoder_settings(project_path),
        }
    }

    /// User-global config dir, used to detect whether the harness is installed on this machine.
    pub fn user_dir(&self) -> Option<PathBuf> {
        match self {
            HookTarget::Claude => DataLayout::claude_user_dir().ok(),
            HookTarget::CodeBuddy => DataLayout::codebuddy_user_dir().ok(),
            HookTarget::WorkBuddy => DataLayout::workbuddy_user_dir().ok(),
            HookTarget::Qoder => DataLayout::qoder_user_dir().ok(),
        }
    }
}

/// Result of registering or revoking the hook across the selected harnesses. Reuses the export
/// `TargetOutcome` vocabulary: register → created | updated | unchanged; revoke → removed | deleted |
/// unchanged.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookReport {
    pub project_path: PathBuf,
    pub targets: Vec<TargetOutcome>,
}

/// Detection state of one harness: whether it appears installed, where its project settings live,
/// and whether our managed hook is already registered there.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookStatus {
    pub target: HookTarget,
    pub label: String,
    pub installed: bool,
    pub settings_path: PathBuf,
    pub registered: bool,
}

/// Locate the bundled hook script. In a release it ships beside the executable; in a dev build the
/// executable lives under `target/<profile>/`, so walk up to the repo's `scripts/` dir. Returns the
/// first existing candidate, or `None` when the script cannot be found.
pub fn locate_hook_script() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut dir = exe.parent()?.to_path_buf();
    for _ in 0..5 {
        for candidate in [dir.join(HOOK_MARKER), dir.join("scripts").join(HOOK_MARKER)] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        dir = dir.parent()?.to_path_buf();
    }
    None
}

/// The default command line that runs the hook script with Node, or `None` when the script is not
/// found (the caller must then supply an explicit command). Forward slashes keep it shell-safe on
/// Windows and avoid backslash escaping inside `settings.json`.
pub fn default_hook_command() -> Option<String> {
    let script = locate_hook_script()?;
    let path = script.display().to_string().replace('\\', "/");
    Some(format!("node \"{path}\""))
}

/// Build the hook group we write for one event. `command` is the full shell command line that runs
/// the hook script. `PreToolUse` carries an empty matcher (matches every tool) so the gate — not the
/// matcher — decides per call; `SessionStart` has no tool matcher.
fn managed_group(command: &str, event: &str) -> Value {
    let mut group = Map::new();
    if event == "PreToolUse" {
        group.insert("matcher".to_string(), json!(""));
    }
    group.insert(
        "hooks".to_string(),
        json!([{ "type": "command", "command": command }]),
    );
    Value::Object(group)
}

/// True when `group` is one of ours: any handler command references the hook script marker.
fn is_managed_group(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|handlers| {
            handlers.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c.contains(HOOK_MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// The managed events currently registered in `settings` (subset of [`HOOK_EVENTS`]). Used by
/// detection; an empty result means our hook is absent.
pub fn registered_events(settings: &Value) -> Vec<&'static str> {
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return Vec::new();
    };
    HOOK_EVENTS
        .iter()
        .filter(|event| {
            hooks
                .get(**event)
                .and_then(Value::as_array)
                .map(|groups| groups.iter().any(is_managed_group))
                .unwrap_or(false)
        })
        .copied()
        .collect()
}

/// Insert or replace our managed hook groups in `settings`, preserving every other key and every
/// user-authored hook group. `settings` may be `Null` (a fresh/empty file). Errors when the file is
/// not a JSON object or `hooks`/event shapes are wrong, rather than risking corruption.
pub fn upsert_managed_hooks(settings: &Value, command: &str) -> Result<Value> {
    let mut root = match settings {
        Value::Object(m) => m.clone(),
        Value::Null => Map::new(),
        _ => bail!("settings.json must be a JSON object"),
    };
    let mut hooks = match root.get("hooks") {
        Some(Value::Object(m)) => m.clone(),
        Some(Value::Null) | None => Map::new(),
        Some(_) => bail!("settings.json 'hooks' must be an object"),
    };
    for event in HOOK_EVENTS {
        let mut groups = match hooks.get(event) {
            Some(Value::Array(a)) => a.clone(),
            Some(Value::Null) | None => Vec::new(),
            Some(_) => bail!("settings.json hooks.{event} must be an array"),
        };
        groups.retain(|g| !is_managed_group(g));
        groups.push(managed_group(command, event));
        hooks.insert(event.to_string(), Value::Array(groups));
    }
    root.insert("hooks".to_string(), Value::Object(hooks));
    Ok(Value::Object(root))
}

/// Remove our managed hook groups from `settings`, preserving everything else. Returns `None` when
/// nothing of ours was present. Empty event arrays and an empty `hooks` object are pruned so no stub
/// is left behind; the caller deletes the file when the result is an empty object it created.
pub fn remove_managed_hooks(settings: &Value) -> Result<Option<Value>> {
    let Some(root) = settings.as_object() else {
        return Ok(None);
    };
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut new_hooks = hooks.clone();
    let mut changed = false;
    for event in HOOK_EVENTS {
        if let Some(Value::Array(groups)) = hooks.get(event) {
            let kept: Vec<Value> = groups
                .iter()
                .filter(|g| !is_managed_group(g))
                .cloned()
                .collect();
            if kept.len() != groups.len() {
                changed = true;
                if kept.is_empty() {
                    new_hooks.remove(event);
                } else {
                    new_hooks.insert(event.to_string(), Value::Array(kept));
                }
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut new_root = root.clone();
    if new_hooks.is_empty() {
        new_root.remove("hooks");
    } else {
        new_root.insert("hooks".to_string(), Value::Object(new_hooks));
    }
    Ok(Some(Value::Object(new_root)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const CMD: &str = "node \"C:/repo/scripts/agent-control-hook.mjs\"";

    fn user_hook() -> Value {
        json!({ "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo user" }] })
    }

    #[test]
    fn upsert_into_empty_settings_creates_both_events() {
        let out = upsert_managed_hooks(&Value::Null, CMD).unwrap();
        let hooks = out.get("hooks").unwrap();
        for event in HOOK_EVENTS {
            let groups = hooks.get(event).unwrap().as_array().unwrap();
            assert_eq!(groups.len(), 1, "{event}");
            assert!(is_managed_group(&groups[0]));
        }
        // PreToolUse matches every tool (empty matcher); SessionStart has none.
        assert_eq!(hooks["PreToolUse"][0]["matcher"], json!(""));
        assert!(hooks["SessionStart"][0].get("matcher").is_none());
        assert_eq!(registered_events(&out), vec!["SessionStart", "PreToolUse"]);
    }

    #[test]
    fn upsert_preserves_other_config_and_user_hooks() {
        let settings = json!({
            "model": "opus",
            "permissions": { "allow": ["Bash"] },
            "hooks": {
                "PreToolUse": [user_hook()],
                "Stop": [user_hook()]
            }
        });
        let out = upsert_managed_hooks(&settings, CMD).unwrap();
        assert_eq!(out["model"], json!("opus"));
        assert_eq!(out["permissions"]["allow"][0], json!("Bash"));
        // The user's PreToolUse group survives alongside ours; Stop is untouched.
        let pre = out["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0], user_hook());
        assert!(is_managed_group(&pre[1]));
        assert_eq!(out["hooks"]["Stop"][0], user_hook());
    }

    #[test]
    fn upsert_is_idempotent_and_does_not_duplicate() {
        let settings = json!({ "hooks": { "PreToolUse": [user_hook()] } });
        let once = upsert_managed_hooks(&settings, CMD).unwrap();
        let twice = upsert_managed_hooks(&once, CMD).unwrap();
        assert_eq!(once, twice);
        assert_eq!(twice["hooks"]["PreToolUse"].as_array().unwrap().len(), 2);
        assert_eq!(twice["hooks"]["SessionStart"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn upsert_replaces_our_group_when_command_changes() {
        let once = upsert_managed_hooks(&Value::Null, CMD).unwrap();
        let new_cmd = "node \"/other/agent-control-hook.mjs\"";
        let twice = upsert_managed_hooks(&once, new_cmd).unwrap();
        let groups = twice["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "our group is replaced, not appended");
        assert_eq!(groups[0]["hooks"][0]["command"], json!(new_cmd));
    }

    #[test]
    fn upsert_rejects_non_object_settings() {
        assert!(upsert_managed_hooks(&json!([]), CMD).is_err());
        assert!(upsert_managed_hooks(&json!({ "hooks": 5 }), CMD).is_err());
        assert!(upsert_managed_hooks(&json!({ "hooks": { "PreToolUse": 5 } }), CMD).is_err());
    }

    #[test]
    fn remove_strips_our_hooks_preserves_user_hooks() {
        let settings = json!({
            "model": "opus",
            "hooks": { "PreToolUse": [user_hook()], "SessionStart": [] }
        });
        let registered = upsert_managed_hooks(&settings, CMD).unwrap();
        let out = remove_managed_hooks(&registered).unwrap().unwrap();
        assert_eq!(out["model"], json!("opus"));
        let pre = out["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0], user_hook());
        // SessionStart held only our group → pruned entirely.
        assert!(out["hooks"].get("SessionStart").is_none());
        assert!(registered_events(&out).is_empty());
    }

    #[test]
    fn remove_prunes_hooks_object_when_ours_only() {
        let registered = upsert_managed_hooks(&Value::Null, CMD).unwrap();
        let out = remove_managed_hooks(&registered).unwrap().unwrap();
        assert_eq!(
            out,
            json!({}),
            "empty settings object → caller deletes file"
        );
    }

    #[test]
    fn remove_returns_none_when_no_managed_hooks() {
        assert_eq!(remove_managed_hooks(&Value::Null).unwrap(), None);
        let settings = json!({ "hooks": { "PreToolUse": [user_hook()] } });
        assert_eq!(remove_managed_hooks(&settings).unwrap(), None);
        assert_eq!(
            remove_managed_hooks(&json!({ "model": "opus" })).unwrap(),
            None
        );
    }

    #[test]
    fn hook_target_parse_paths_and_labels() {
        for (s, t) in [
            ("claude", HookTarget::Claude),
            ("Claude-Code", HookTarget::Claude),
            ("codebuddy", HookTarget::CodeBuddy),
            ("workbuddy", HookTarget::WorkBuddy),
            ("WorkBuddy", HookTarget::WorkBuddy),
            ("qoder", HookTarget::Qoder),
        ] {
            assert_eq!(HookTarget::parse(s), Some(t), "parse {s}");
        }
        assert_eq!(HookTarget::parse("bogus"), None);

        let p = Path::new("C:/proj");
        assert_eq!(
            HookTarget::Claude.settings_path(p),
            p.join(".claude").join("settings.json")
        );
        assert_eq!(
            HookTarget::CodeBuddy.settings_path(p),
            p.join(".codebuddy").join("settings.json")
        );
        assert_eq!(
            HookTarget::WorkBuddy.settings_path(p),
            p.join(".workbuddy").join("settings.json")
        );
        assert_eq!(
            HookTarget::Qoder.settings_path(p),
            p.join(".qoder").join("settings.json")
        );
        assert_eq!(HookTarget::CodeBuddy.label(), "CodeBuddy");
        assert_eq!(HookTarget::WorkBuddy.label(), "WorkBuddy");
        assert_eq!(HookTarget::ALL.len(), 4);
    }

    #[test]
    fn is_managed_group_matches_only_our_command() {
        assert!(is_managed_group(&managed_group(CMD, "PreToolUse")));
        assert!(!is_managed_group(&user_hook()));
        assert!(!is_managed_group(&json!({ "hooks": [] })));
        assert!(!is_managed_group(&json!({})));
    }
}
