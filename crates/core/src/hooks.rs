//! Register the Agent Control hook into each harness's config so the harness itself drives the
//! SessionStart briefing (reach) and the PreToolUse gate (enforce).
//!
//! This is the config counterpart of [`crate::export`]'s managed block: we upsert/remove only *our*
//! hook entries and preserve every other key and every user-authored hook. A group is "ours" when one
//! of its commands references [`HOOK_MARKER`] (the hook script basename), so no extra marker field is
//! needed that a harness might reject. Registering is idempotent; revoking prunes the events/hooks we
//! emptied and leaves the rest of the file untouched.
//!
//! Nine harnesses are supported through four config codecs. Most vendors (Claude Code, CodeBuddy,
//! WorkBuddy, Qoder, Qoder CN, Codex, Kimi) share the Claude-shaped nested-JSON / TOML schema and the
//! exit-2 + stderr deny signal, so the hook script runs unchanged for them. Cursor (flat JSON, must
//! emit an explicit `{permission:"allow"}`) and Antigravity (named bundle, silence == deny) differ in
//! both config shape and script I/O; the script selects their behavior from the vendor id we bake into
//! the registered command.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};

use crate::config::DataLayout;
use crate::export::TargetOutcome;

/// Substring that identifies a hook entry as managed by Agent Control: any entry whose command
/// invokes our hook script is ours. Everything else belongs to the user and is preserved.
pub const HOOK_MARKER: &str = "agent-control-hook.mjs";

/// Host-level backstop (seconds) independent of the script's own deadline, so a hung hook is killed
/// by the harness even if the script's guard timer never fires.
const HOOK_TIMEOUT_SECONDS: u64 = 10;

/// Named bundle key Antigravity groups our hooks under inside `~/.gemini/config/hooks.json`.
const ANTIGRAVITY_BUNDLE: &str = "agent-control";

/// Marker comments delimiting our managed `[[hooks]]` block inside Kimi's `config.toml`. TOML cannot
/// be round-tripped through a parser without losing the user's comments/formatting, so we own a
/// comment-delimited text region instead (the TOML analogue of export's managed block).
const TOML_START: &str = "# >>> agent-control hooks (managed by Agent Control; do not edit) >>>";
const TOML_END: &str = "# <<< agent-control hooks <<<";

/// The on-disk config format a harness uses for its hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCodec {
    /// `{"hooks": {Event: [{matcher?, hooks: [{type, command, timeout}]}]}}` — Claude family, Codex.
    NestedJson,
    /// `{"version": 1, "hooks": {event: [{command, timeout}]}}` — Cursor (command on the definition).
    FlatJson,
    /// `{"<bundle>": {Event: [{matcher, hooks: [...]} | {type, command, timeout}]}}` — Antigravity.
    NamedBundleJson,
    /// `[[hooks]]` tables inside a user TOML config, owned via a marker block — Kimi.
    Toml,
}

/// Where a harness's hook config is written. Project-scope vendors keep the hook in the repo (shared,
/// committed); user-only vendors (Kimi, Antigravity) write a user-global file. A user-level hook still
/// only enforces for the matching project because the gate resolves rules by the call's `cwd`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookScope {
    Project,
    User,
}

/// The two event names a vendor uses for reach (briefing) and enforce (gate). Most vendors use
/// `SessionStart`/`PreToolUse`; Cursor lowercases them; Antigravity gates on `PreToolUse` but starts a
/// turn on `PreInvocation`.
#[derive(Debug, Clone, Copy)]
pub struct HookEvents {
    pub start: &'static str,
    pub gate: &'static str,
}

const CLAUDE_EVENTS: HookEvents = HookEvents {
    start: "SessionStart",
    gate: "PreToolUse",
};

/// A harness whose hook config we can register into. CodeBuddy and WorkBuddy are separate products
/// (WorkBuddy ships the same engine rebranded); Qoder and Qoder CN are the international and China
/// builds with separate config homes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HookTarget {
    Claude,
    CodeBuddy,
    WorkBuddy,
    Qoder,
    #[serde(rename = "qoder-cn")]
    QoderCn,
    Codex,
    Cursor,
    Antigravity,
    Kimi,
}

impl HookTarget {
    pub const ALL: [HookTarget; 9] = [
        HookTarget::Claude,
        HookTarget::CodeBuddy,
        HookTarget::WorkBuddy,
        HookTarget::Qoder,
        HookTarget::QoderCn,
        HookTarget::Codex,
        HookTarget::Cursor,
        HookTarget::Antigravity,
        HookTarget::Kimi,
    ];

    /// Parse a user/MCP-supplied target name, case-insensitive.
    pub fn parse(s: &str) -> Option<HookTarget> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" | "claude code" => Some(HookTarget::Claude),
            "codebuddy" => Some(HookTarget::CodeBuddy),
            "workbuddy" => Some(HookTarget::WorkBuddy),
            "qoder" => Some(HookTarget::Qoder),
            "qoder-cn" | "qoder cn" | "qodercn" => Some(HookTarget::QoderCn),
            "codex" => Some(HookTarget::Codex),
            "cursor" => Some(HookTarget::Cursor),
            "antigravity" | "antigravity-cli" => Some(HookTarget::Antigravity),
            "kimi" | "kimi-code" | "kimi code" => Some(HookTarget::Kimi),
            _ => None,
        }
    }

    pub fn key(&self) -> &'static str {
        match self {
            HookTarget::Claude => "claude",
            HookTarget::CodeBuddy => "codebuddy",
            HookTarget::WorkBuddy => "workbuddy",
            HookTarget::Qoder => "qoder",
            HookTarget::QoderCn => "qoder-cn",
            HookTarget::Codex => "codex",
            HookTarget::Cursor => "cursor",
            HookTarget::Antigravity => "antigravity",
            HookTarget::Kimi => "kimi",
        }
    }

    /// Human-readable harness name for the console / reports.
    pub fn label(&self) -> &'static str {
        match self {
            HookTarget::Claude => "Claude Code",
            HookTarget::CodeBuddy => "CodeBuddy",
            HookTarget::WorkBuddy => "WorkBuddy",
            HookTarget::Qoder => "Qoder",
            HookTarget::QoderCn => "Qoder CN",
            HookTarget::Codex => "Codex",
            HookTarget::Cursor => "Cursor",
            HookTarget::Antigravity => "Antigravity",
            HookTarget::Kimi => "Kimi Code",
        }
    }

    /// Vendor id baked into the registered command so the hook script can select stdin normalization
    /// and deny/allow output. The Claude-shaped vendors all share the script's default behavior.
    pub fn vendor_id(&self) -> &'static str {
        self.key()
    }

    pub fn codec(&self) -> ConfigCodec {
        match self {
            HookTarget::Claude
            | HookTarget::CodeBuddy
            | HookTarget::WorkBuddy
            | HookTarget::Qoder
            | HookTarget::QoderCn
            | HookTarget::Codex => ConfigCodec::NestedJson,
            HookTarget::Cursor => ConfigCodec::FlatJson,
            HookTarget::Antigravity => ConfigCodec::NamedBundleJson,
            HookTarget::Kimi => ConfigCodec::Toml,
        }
    }

    pub fn scope(&self) -> HookScope {
        match self {
            HookTarget::Kimi | HookTarget::Antigravity => HookScope::User,
            _ => HookScope::Project,
        }
    }

    pub fn events(&self) -> HookEvents {
        match self {
            HookTarget::Cursor => HookEvents {
                start: "sessionStart",
                gate: "preToolUse",
            },
            HookTarget::Antigravity => HookEvents {
                start: "PreInvocation",
                gate: "PreToolUse",
            },
            _ => CLAUDE_EVENTS,
        }
    }

    /// The config file we read/write for this target: project-relative for project-scope vendors,
    /// user-global for the user-only vendors (Kimi, Antigravity).
    pub fn config_path(&self, project_path: &Path) -> Result<PathBuf> {
        let path = match self {
            HookTarget::Claude => DataLayout::project_claude_settings(project_path),
            HookTarget::CodeBuddy => DataLayout::project_codebuddy_settings(project_path),
            HookTarget::WorkBuddy => DataLayout::project_workbuddy_settings(project_path),
            HookTarget::Qoder => DataLayout::project_qoder_settings(project_path),
            HookTarget::QoderCn => DataLayout::project_qoder_cn_settings(project_path),
            HookTarget::Codex => DataLayout::project_codex_hooks(project_path),
            HookTarget::Cursor => DataLayout::project_cursor_hooks(project_path),
            HookTarget::Kimi => DataLayout::kimi_user_config()?,
            HookTarget::Antigravity => DataLayout::antigravity_user_hooks()?,
        };
        Ok(path)
    }

    /// User-global config dir, used to detect whether the harness is installed on this machine.
    pub fn user_dir(&self) -> Option<PathBuf> {
        match self {
            HookTarget::Claude => DataLayout::claude_user_dir().ok(),
            HookTarget::CodeBuddy => DataLayout::codebuddy_user_dir().ok(),
            HookTarget::WorkBuddy => DataLayout::workbuddy_user_dir().ok(),
            HookTarget::Qoder => DataLayout::qoder_user_dir().ok(),
            HookTarget::QoderCn => DataLayout::qoder_cn_user_dir().ok(),
            HookTarget::Codex => DataLayout::codex_user_dir().ok(),
            HookTarget::Cursor => DataLayout::cursor_user_dir().ok(),
            HookTarget::Kimi => DataLayout::kimi_user_dir().ok(),
            HookTarget::Antigravity => DataLayout::antigravity_user_dir().ok(),
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

/// Detection state of one harness: whether it appears installed, where its config lives, its write
/// scope, and whether our managed hook is already registered there.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HookStatus {
    pub target: HookTarget,
    pub label: String,
    pub scope: HookScope,
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

/// The base command line that runs the hook script with Node, or `None` when the script is not found
/// (the caller must then supply an explicit command). Forward slashes keep it shell-safe on Windows
/// and avoid backslash escaping inside the config. The vendor id and event are appended per entry by
/// [`upsert`] so the script knows which harness it is serving.
pub fn default_hook_command() -> Option<String> {
    let script = locate_hook_script()?;
    let path = script.display().to_string().replace('\\', "/");
    Some(format!("node \"{path}\""))
}

/// The full command for one event: the base invocation plus the vendor id and event name. The script
/// reads the vendor/event from argv (authoritative even when a harness omits `hook_event_name` in
/// stdin) and falls back to stdin when they are absent.
fn entry_command(base: &str, target: HookTarget, event: &str) -> String {
    format!("{base} {} {event}", target.vendor_id())
}

/// Insert or replace our managed hook entries in `existing` config text for `target`, preserving all
/// other content. `base_command` is the script invocation (vendor/event are appended per entry).
/// Returns the new file text. Errors on malformed config rather than risking corruption.
pub fn upsert(target: HookTarget, existing: &str, base_command: &str) -> Result<String> {
    let events = target.events();
    let cmd = |event: &str| entry_command(base_command, target, event);
    match target.codec() {
        ConfigCodec::NestedJson => {
            let value = nested_upsert(&parse_json(existing)?, &cmd, &events)?;
            json_text(&value)
        }
        ConfigCodec::FlatJson => {
            let value = flat_upsert(&parse_json(existing)?, &cmd, &events)?;
            json_text(&value)
        }
        ConfigCodec::NamedBundleJson => {
            let value = bundle_upsert(&parse_json(existing)?, &cmd, &events)?;
            json_text(&value)
        }
        ConfigCodec::Toml => Ok(toml_upsert(existing, &cmd, &events)),
    }
}

/// Remove our managed hook entries from `existing` config text. Returns `None` when nothing of ours
/// was present; `Some(text)` is the new content (an empty string or `{}` means the file held only our
/// hook and the caller should delete it).
pub fn remove(target: HookTarget, existing: &str) -> Result<Option<String>> {
    let events = target.events();
    match target.codec() {
        ConfigCodec::NestedJson => Ok(nested_remove(&parse_json(existing)?, &events)?
            .map(|v| json_text(&v))
            .transpose()?),
        ConfigCodec::FlatJson => Ok(flat_remove(&parse_json(existing)?, &events)?
            .map(|v| json_text(&v))
            .transpose()?),
        ConfigCodec::NamedBundleJson => Ok(bundle_remove(&parse_json(existing)?, &events)?
            .map(|v| json_text(&v))
            .transpose()?),
        ConfigCodec::Toml => Ok(toml_remove(existing)),
    }
}

/// True when our managed hook is registered in `existing` config text for `target`.
pub fn registered(target: HookTarget, existing: &str) -> bool {
    let events = target.events();
    let parsed = match target.codec() {
        ConfigCodec::Toml => return !toml_managed_regions(existing).is_empty(),
        _ => match parse_json(existing) {
            Ok(v) => v,
            Err(_) => return false,
        },
    };
    match target.codec() {
        ConfigCodec::NestedJson => nested_registered(&parsed, &events),
        ConfigCodec::FlatJson => flat_registered(&parsed, &events),
        ConfigCodec::NamedBundleJson => bundle_registered(&parsed, &events),
        ConfigCodec::Toml => unreachable!(),
    }
}

// ---- shared JSON helpers ---------------------------------------------------------------------

fn parse_json(existing: &str) -> Result<Value> {
    if existing.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(existing).context("parsing hook config JSON")
}

fn json_text(value: &Value) -> Result<String> {
    let mut text = serde_json::to_string_pretty(value).context("serializing hook config")?;
    text.push('\n');
    Ok(text)
}

fn command_hook(command: &str) -> Value {
    json!({ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECONDS })
}

fn command_is_managed(command: Option<&Value>) -> bool {
    command
        .and_then(Value::as_str)
        .map(|c| c.contains(HOOK_MARKER))
        .unwrap_or(false)
}

/// True when a nested group's `hooks` array contains one of our commands.
fn hooks_array_managed(group: &Value) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map(|handlers| {
            handlers
                .iter()
                .any(|h| command_is_managed(h.get("command")))
        })
        .unwrap_or(false)
}

/// Take the root object (or a fresh map for `Null`); error on any other shape.
fn root_object(settings: &Value) -> Result<Map<String, Value>> {
    match settings {
        Value::Object(m) => Ok(m.clone()),
        Value::Null => Ok(Map::new()),
        _ => bail!("hook config must be a JSON object"),
    }
}

/// Take a named object field (or a fresh map when absent/null); error on a non-object value.
fn child_object(root: &Map<String, Value>, key: &str) -> Result<Map<String, Value>> {
    match root.get(key) {
        Some(Value::Object(m)) => Ok(m.clone()),
        Some(Value::Null) | None => Ok(Map::new()),
        Some(_) => bail!("hook config '{key}' must be an object"),
    }
}

/// Take an event's array of groups (or a fresh vec when absent/null); error on a non-array value.
fn event_groups(hooks: &Map<String, Value>, event: &str) -> Result<Vec<Value>> {
    match hooks.get(event) {
        Some(Value::Array(a)) => Ok(a.clone()),
        Some(Value::Null) | None => Ok(Vec::new()),
        Some(_) => bail!("hook config event '{event}' must be an array"),
    }
}

// ---- NestedJson (Claude family, Codex, Qoder CN) ---------------------------------------------

fn nested_group(command: &str, is_gate: bool) -> Value {
    let mut group = Map::new();
    if is_gate {
        group.insert("matcher".to_string(), json!(""));
    }
    group.insert("hooks".to_string(), json!([command_hook(command)]));
    Value::Object(group)
}

fn nested_upsert(
    settings: &Value,
    cmd: &dyn Fn(&str) -> String,
    events: &HookEvents,
) -> Result<Value> {
    let mut root = root_object(settings)?;
    let mut hooks = child_object(&root, "hooks")?;
    for event in [events.start, events.gate] {
        let mut groups = event_groups(&hooks, event)?;
        groups.retain(|g| !hooks_array_managed(g));
        groups.push(nested_group(&cmd(event), event == events.gate));
        hooks.insert(event.to_string(), Value::Array(groups));
    }
    root.insert("hooks".to_string(), Value::Object(hooks));
    Ok(Value::Object(root))
}

fn nested_remove(settings: &Value, events: &HookEvents) -> Result<Option<Value>> {
    let Some(root) = settings.as_object() else {
        return Ok(None);
    };
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut new_hooks = hooks.clone();
    let mut changed = false;
    for event in [events.start, events.gate] {
        if let Some(Value::Array(groups)) = hooks.get(event) {
            let kept: Vec<Value> = groups
                .iter()
                .filter(|g| !hooks_array_managed(g))
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

fn nested_registered(settings: &Value, events: &HookEvents) -> bool {
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    [events.start, events.gate].iter().any(|event| {
        hooks
            .get(*event)
            .and_then(Value::as_array)
            .map(|groups| groups.iter().any(hooks_array_managed))
            .unwrap_or(false)
    })
}

// ---- FlatJson (Cursor) -----------------------------------------------------------------------

fn flat_definition(command: &str) -> Value {
    json!({ "command": command, "timeout": HOOK_TIMEOUT_SECONDS })
}

fn flat_upsert(
    settings: &Value,
    cmd: &dyn Fn(&str) -> String,
    events: &HookEvents,
) -> Result<Value> {
    let mut root = root_object(settings)?;
    root.insert("version".to_string(), json!(1));
    let mut hooks = child_object(&root, "hooks")?;
    for event in [events.start, events.gate] {
        let mut groups = event_groups(&hooks, event)?;
        groups.retain(|g| !command_is_managed(g.get("command")));
        groups.push(flat_definition(&cmd(event)));
        hooks.insert(event.to_string(), Value::Array(groups));
    }
    root.insert("hooks".to_string(), Value::Object(hooks));
    Ok(Value::Object(root))
}

fn flat_remove(settings: &Value, events: &HookEvents) -> Result<Option<Value>> {
    let Some(root) = settings.as_object() else {
        return Ok(None);
    };
    let Some(hooks) = root.get("hooks").and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut new_hooks = hooks.clone();
    let mut changed = false;
    for event in [events.start, events.gate] {
        if let Some(Value::Array(groups)) = hooks.get(event) {
            let kept: Vec<Value> = groups
                .iter()
                .filter(|g| !command_is_managed(g.get("command")))
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
        // `version` alone is our stub; drop it so the caller deletes an otherwise-empty file.
        if new_root.get("version").and_then(Value::as_u64) == Some(1) && new_root.len() == 1 {
            new_root.remove("version");
        }
    } else {
        new_root.insert("hooks".to_string(), Value::Object(new_hooks));
    }
    Ok(Some(Value::Object(new_root)))
}

fn flat_registered(settings: &Value, events: &HookEvents) -> bool {
    let Some(hooks) = settings.get("hooks").and_then(Value::as_object) else {
        return false;
    };
    [events.start, events.gate].iter().any(|event| {
        hooks
            .get(*event)
            .and_then(Value::as_array)
            .map(|groups| groups.iter().any(|g| command_is_managed(g.get("command"))))
            .unwrap_or(false)
    })
}

// ---- NamedBundleJson (Antigravity) -----------------------------------------------------------

/// Antigravity gates on a tool-schema event (`{matcher:"*", hooks:[...]}`) and starts a turn on a
/// direct-schema event (`{type:"command", command, timeout}`).
fn bundle_entry(command: &str, is_gate: bool) -> Value {
    if is_gate {
        json!({ "matcher": "*", "hooks": [command_hook(command)] })
    } else {
        json!({ "type": "command", "command": command, "timeout": HOOK_TIMEOUT_SECONDS })
    }
}

fn bundle_entry_managed(entry: &Value) -> bool {
    command_is_managed(entry.get("command")) || hooks_array_managed(entry)
}

fn bundle_upsert(
    settings: &Value,
    cmd: &dyn Fn(&str) -> String,
    events: &HookEvents,
) -> Result<Value> {
    let mut root = root_object(settings)?;
    let mut bundle = child_object(&root, ANTIGRAVITY_BUNDLE)?;
    for event in [events.start, events.gate] {
        let mut groups = event_groups(&bundle, event)?;
        groups.retain(|g| !bundle_entry_managed(g));
        groups.push(bundle_entry(&cmd(event), event == events.gate));
        bundle.insert(event.to_string(), Value::Array(groups));
    }
    root.insert(ANTIGRAVITY_BUNDLE.to_string(), Value::Object(bundle));
    Ok(Value::Object(root))
}

fn bundle_remove(settings: &Value, events: &HookEvents) -> Result<Option<Value>> {
    let Some(root) = settings.as_object() else {
        return Ok(None);
    };
    let Some(bundle) = root.get(ANTIGRAVITY_BUNDLE).and_then(Value::as_object) else {
        return Ok(None);
    };
    let mut new_bundle = bundle.clone();
    let mut changed = false;
    for event in [events.start, events.gate] {
        if let Some(Value::Array(groups)) = bundle.get(event) {
            let kept: Vec<Value> = groups
                .iter()
                .filter(|g| !bundle_entry_managed(g))
                .cloned()
                .collect();
            if kept.len() != groups.len() {
                changed = true;
                if kept.is_empty() {
                    new_bundle.remove(event);
                } else {
                    new_bundle.insert(event.to_string(), Value::Array(kept));
                }
            }
        }
    }
    if !changed {
        return Ok(None);
    }
    let mut new_root = root.clone();
    if new_bundle.is_empty() {
        new_root.remove(ANTIGRAVITY_BUNDLE);
    } else {
        new_root.insert(ANTIGRAVITY_BUNDLE.to_string(), Value::Object(new_bundle));
    }
    Ok(Some(Value::Object(new_root)))
}

fn bundle_registered(settings: &Value, events: &HookEvents) -> bool {
    let Some(bundle) = settings.get(ANTIGRAVITY_BUNDLE).and_then(Value::as_object) else {
        return false;
    };
    [events.start, events.gate].iter().any(|event| {
        bundle
            .get(*event)
            .and_then(Value::as_array)
            .map(|groups| groups.iter().any(bundle_entry_managed))
            .unwrap_or(false)
    })
}

// ---- Toml (Kimi) -----------------------------------------------------------------------------

fn toml_basic_string(value: &str) -> String {
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    format!("\"{escaped}\"")
}

fn detect_eol(text: &str) -> &'static str {
    if text.contains("\r\n") { "\r\n" } else { "\n" }
}

/// Build our managed `[[hooks]]` block (marker-delimited) for the vendor's events.
fn toml_block(cmd: &dyn Fn(&str) -> String, events: &HookEvents, eol: &str) -> String {
    let mut lines = vec![TOML_START.to_string()];
    for event in [events.start, events.gate] {
        lines.push("[[hooks]]".to_string());
        lines.push(format!("event = {}", toml_basic_string(event)));
        lines.push(format!("command = {}", toml_basic_string(&cmd(event))));
        lines.push(format!("timeout = {HOOK_TIMEOUT_SECONDS}"));
        lines.push(String::new());
    }
    // Drop the trailing blank entry separator, then close the marker.
    lines.pop();
    lines.push(TOML_END.to_string());
    lines.join(eol)
}

/// Line ranges `[start, end)` we own: marker-delimited blocks plus any positively-recognized managed
/// `[[hooks]]` table (so revoke still works if a user moved our tables out of the markers).
fn toml_managed_regions(text: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut regions: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if trimmed == TOML_START {
            // Marker block: own through the matching end marker (inclusive).
            let mut end = i + 1;
            while end < lines.len() && lines[end].trim() != TOML_END {
                end += 1;
            }
            let end = if end < lines.len() { end + 1 } else { end };
            regions.push((i, end));
            i = end;
            continue;
        }
        if trimmed == "[[hooks]]" {
            let mut end = i + 1;
            let mut managed = false;
            while end < lines.len() {
                let t = lines[end].trim();
                if t.is_empty() || t.starts_with('[') || t.starts_with('#') {
                    break;
                }
                if t.starts_with("command") && t.contains(HOOK_MARKER) {
                    managed = true;
                }
                end += 1;
            }
            if managed {
                regions.push((i, end));
                i = end;
                continue;
            }
        }
        i += 1;
    }
    regions
}

/// Remove the owned line ranges, returning the surviving text and whether anything was removed.
fn toml_strip(text: &str) -> (String, bool) {
    let regions = toml_managed_regions(text);
    if regions.is_empty() {
        return (text.to_string(), false);
    }
    let lines: Vec<&str> = text.lines().collect();
    let mut kept: Vec<&str> = Vec::new();
    let mut i = 0;
    let mut r = 0;
    while i < lines.len() {
        if r < regions.len() && i == regions[r].0 {
            i = regions[r].1;
            r += 1;
            continue;
        }
        kept.push(lines[i]);
        i += 1;
    }
    (kept.join("\n"), true)
}

fn toml_upsert(existing: &str, cmd: &dyn Fn(&str) -> String, events: &HookEvents) -> String {
    let eol = detect_eol(existing);
    let (stripped, _) = toml_strip(existing);
    let body = stripped.trim_end();
    let block = toml_block(cmd, events, eol);
    if body.is_empty() {
        format!("{block}{eol}")
    } else {
        format!("{body}{eol}{eol}{block}{eol}")
    }
}

fn toml_remove(existing: &str) -> Option<String> {
    let (stripped, changed) = toml_strip(existing);
    if !changed {
        return None;
    }
    let eol = detect_eol(existing);
    let body = stripped.trim_end();
    Some(if body.is_empty() {
        String::new()
    } else {
        format!("{body}{eol}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "node \"C:/repo/scripts/agent-control-hook.mjs\"";

    fn user_hook() -> Value {
        json!({ "matcher": "Bash", "hooks": [{ "type": "command", "command": "echo user" }] })
    }

    fn cmd_for(target: HookTarget, event: &str) -> String {
        entry_command(BASE, target, event)
    }

    // ---- NestedJson (Claude) ----

    #[test]
    fn nested_upsert_into_empty_creates_both_events() {
        let out = upsert(HookTarget::Claude, "", BASE).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let hooks = &v["hooks"];
        assert!(hooks["SessionStart"][0].get("matcher").is_none());
        assert_eq!(hooks["PreToolUse"][0]["matcher"], json!(""));
        assert_eq!(
            hooks["PreToolUse"][0]["hooks"][0]["command"],
            json!(cmd_for(HookTarget::Claude, "PreToolUse"))
        );
        assert!(registered(HookTarget::Claude, &out));
    }

    #[test]
    fn nested_preserves_other_config_and_user_hooks() {
        let existing = json!({
            "model": "opus",
            "permissions": { "allow": ["Bash"] },
            "hooks": { "PreToolUse": [user_hook()], "Stop": [user_hook()] }
        })
        .to_string();
        let out = upsert(HookTarget::Claude, &existing, BASE).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], json!("opus"));
        assert_eq!(v["permissions"]["allow"][0], json!("Bash"));
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0], user_hook());
        assert!(hooks_array_managed(&pre[1]));
        assert_eq!(v["hooks"]["Stop"][0], user_hook());
    }

    #[test]
    fn nested_is_idempotent() {
        let once = upsert(HookTarget::Claude, "", BASE).unwrap();
        let twice = upsert(HookTarget::Claude, &once, BASE).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn nested_replaces_our_group_when_command_changes() {
        let once = upsert(HookTarget::Claude, "", BASE).unwrap();
        let new_base = "node \"/other/agent-control-hook.mjs\"";
        let twice = upsert(HookTarget::Claude, &once, new_base).unwrap();
        let v: Value = serde_json::from_str(&twice).unwrap();
        let groups = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(groups.len(), 1, "our group is replaced, not appended");
        assert_eq!(
            groups[0]["hooks"][0]["command"],
            json!(entry_command(new_base, HookTarget::Claude, "PreToolUse"))
        );
    }

    #[test]
    fn nested_remove_strips_ours_preserves_user() {
        let existing =
            json!({ "model": "opus", "hooks": { "PreToolUse": [user_hook()] } }).to_string();
        let reg = upsert(HookTarget::Claude, &existing, BASE).unwrap();
        let out = remove(HookTarget::Claude, &reg).unwrap().unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], json!("opus"));
        let pre = v["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 1);
        assert_eq!(pre[0], user_hook());
        assert!(v["hooks"].get("SessionStart").is_none());
        assert!(!registered(HookTarget::Claude, &out));
    }

    #[test]
    fn nested_remove_prunes_to_empty_object() {
        let registered = upsert(HookTarget::Claude, "", BASE).unwrap();
        let out = remove(HookTarget::Claude, &registered).unwrap().unwrap();
        assert_eq!(out.trim(), "{}");
    }

    #[test]
    fn nested_remove_none_when_absent() {
        assert_eq!(remove(HookTarget::Claude, "").unwrap(), None);
        let user = json!({ "hooks": { "PreToolUse": [user_hook()] } }).to_string();
        assert_eq!(remove(HookTarget::Claude, &user).unwrap(), None);
        assert!(!registered(HookTarget::Claude, &user));
    }

    #[test]
    fn nested_rejects_non_object() {
        assert!(upsert(HookTarget::Claude, "[]", BASE).is_err());
        assert!(upsert(HookTarget::Claude, "{\"hooks\":5}", BASE).is_err());
        assert!(upsert(HookTarget::Claude, "{\"hooks\":{\"PreToolUse\":5}}", BASE).is_err());
    }

    #[test]
    fn codex_uses_nested_hooks_json_shape() {
        let out = upsert(HookTarget::Codex, "", BASE).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert!(v["hooks"]["SessionStart"].is_array());
        assert!(v["hooks"]["PreToolUse"].is_array());
        assert_eq!(
            v["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            json!(cmd_for(HookTarget::Codex, "PreToolUse"))
        );
    }

    // ---- FlatJson (Cursor) ----

    #[test]
    fn cursor_flat_version_and_command_on_definition() {
        let out = upsert(HookTarget::Cursor, "", BASE).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["version"], json!(1));
        let pre = &v["hooks"]["preToolUse"][0];
        assert_eq!(
            pre["command"],
            json!(cmd_for(HookTarget::Cursor, "preToolUse"))
        );
        assert!(
            pre.get("hooks").is_none(),
            "Cursor puts command on the definition"
        );
        assert!(v["hooks"]["sessionStart"].is_array());
        assert!(registered(HookTarget::Cursor, &out));
    }

    #[test]
    fn cursor_preserves_user_entries_and_is_idempotent() {
        let existing =
            json!({ "version": 1, "hooks": { "preToolUse": [{ "command": "echo user" }] } })
                .to_string();
        let once = upsert(HookTarget::Cursor, &existing, BASE).unwrap();
        let twice = upsert(HookTarget::Cursor, &once, BASE).unwrap();
        assert_eq!(once, twice);
        let v: Value = serde_json::from_str(&once).unwrap();
        let pre = v["hooks"]["preToolUse"].as_array().unwrap();
        assert_eq!(pre.len(), 2);
        assert_eq!(pre[0]["command"], json!("echo user"));
    }

    #[test]
    fn cursor_remove_drops_version_stub() {
        let registered = upsert(HookTarget::Cursor, "", BASE).unwrap();
        let out = remove(HookTarget::Cursor, &registered).unwrap().unwrap();
        assert_eq!(
            out.trim(),
            "{}",
            "version-only stub is removed so the file is deleted"
        );
    }

    // ---- NamedBundleJson (Antigravity) ----

    #[test]
    fn antigravity_bundle_tool_and_direct_schemas() {
        let out = upsert(HookTarget::Antigravity, "", BASE).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let bundle = &v[ANTIGRAVITY_BUNDLE];
        // Gate is tool-schema with matcher "*"; start is a direct command definition.
        assert_eq!(bundle["PreToolUse"][0]["matcher"], json!("*"));
        assert_eq!(
            bundle["PreToolUse"][0]["hooks"][0]["command"],
            json!(cmd_for(HookTarget::Antigravity, "PreToolUse"))
        );
        assert_eq!(bundle["PreInvocation"][0]["type"], json!("command"));
        assert!(bundle["PreInvocation"][0].get("hooks").is_none());
        assert!(registered(HookTarget::Antigravity, &out));
    }

    #[test]
    fn antigravity_preserves_other_bundles_and_removes_cleanly() {
        let existing =
            json!({ "other-bundle": { "Stop": [{ "type": "command", "command": "x" }] } })
                .to_string();
        let reg = upsert(HookTarget::Antigravity, &existing, BASE).unwrap();
        let v: Value = serde_json::from_str(&reg).unwrap();
        assert_eq!(v["other-bundle"]["Stop"][0]["command"], json!("x"));
        let out = remove(HookTarget::Antigravity, &reg).unwrap().unwrap();
        let v2: Value = serde_json::from_str(&out).unwrap();
        assert!(v2.get(ANTIGRAVITY_BUNDLE).is_none());
        assert_eq!(v2["other-bundle"]["Stop"][0]["command"], json!("x"));
        assert!(!registered(HookTarget::Antigravity, &out));
    }

    // ---- Toml (Kimi) ----

    #[test]
    fn kimi_toml_block_has_only_allowed_fields() {
        let out = upsert(HookTarget::Kimi, "", BASE).unwrap();
        assert!(out.contains(TOML_START));
        assert!(out.contains(TOML_END));
        assert!(out.contains("[[hooks]]"));
        assert!(out.contains("event = \"SessionStart\""));
        assert!(out.contains("event = \"PreToolUse\""));
        assert!(out.contains("timeout = 10"));
        assert!(out.contains(HOOK_MARKER));
        // Kimi rejects unknown fields; we must emit only event/command/timeout.
        assert!(!out.contains("matcher"));
        assert!(registered(HookTarget::Kimi, &out));
    }

    #[test]
    fn kimi_toml_preserves_user_config_and_is_idempotent() {
        let existing = "# user config\nmodel = \"kimi\"\n\n[[permission.rules]]\ndecision = \"deny\"\npattern = \"Bash(rm -rf*)\"\n";
        let once = upsert(HookTarget::Kimi, existing, BASE).unwrap();
        let twice = upsert(HookTarget::Kimi, &once, BASE).unwrap();
        assert_eq!(once, twice);
        assert!(once.contains("model = \"kimi\""));
        assert!(once.contains("[[permission.rules]]"));
        assert!(once.contains("# user config"));
    }

    #[test]
    fn kimi_toml_remove_restores_user_bytes() {
        let existing = "model = \"kimi\"\n";
        let reg = upsert(HookTarget::Kimi, existing, BASE).unwrap();
        let out = remove(HookTarget::Kimi, &reg).unwrap().unwrap();
        assert_eq!(out, "model = \"kimi\"\n");
        assert!(!registered(HookTarget::Kimi, &out));
    }

    #[test]
    fn kimi_toml_remove_owns_tables_outside_markers() {
        // A managed table with no markers (user moved it) is still recognized and removed.
        let text = format!(
            "model = \"kimi\"\n\n[[hooks]]\nevent = \"PreToolUse\"\ncommand = {}\ntimeout = 10\n",
            toml_basic_string(&cmd_for(HookTarget::Kimi, "PreToolUse"))
        );
        assert!(registered(HookTarget::Kimi, &text));
        let out = remove(HookTarget::Kimi, &text).unwrap().unwrap();
        assert_eq!(out, "model = \"kimi\"\n");
    }

    #[test]
    fn kimi_toml_remove_none_when_absent() {
        assert_eq!(
            remove(HookTarget::Kimi, "model = \"kimi\"\n").unwrap(),
            None
        );
        assert!(!registered(HookTarget::Kimi, "model = \"kimi\"\n"));
    }

    // ---- descriptors ----

    #[test]
    fn target_parse_keys_labels_and_scopes() {
        for (s, t) in [
            ("claude", HookTarget::Claude),
            ("Claude-Code", HookTarget::Claude),
            ("codebuddy", HookTarget::CodeBuddy),
            ("workbuddy", HookTarget::WorkBuddy),
            ("qoder", HookTarget::Qoder),
            ("qoder-cn", HookTarget::QoderCn),
            ("Qoder CN", HookTarget::QoderCn),
            ("codex", HookTarget::Codex),
            ("cursor", HookTarget::Cursor),
            ("antigravity", HookTarget::Antigravity),
            ("kimi", HookTarget::Kimi),
            ("kimi-code", HookTarget::Kimi),
        ] {
            assert_eq!(HookTarget::parse(s), Some(t), "parse {s}");
        }
        assert_eq!(HookTarget::parse("bogus"), None);
        assert_eq!(HookTarget::ALL.len(), 9);

        assert_eq!(HookTarget::Qoder.scope(), HookScope::Project);
        assert_eq!(HookTarget::Kimi.scope(), HookScope::User);
        assert_eq!(HookTarget::Antigravity.scope(), HookScope::User);
        assert_eq!(HookTarget::Cursor.codec(), ConfigCodec::FlatJson);
        assert_eq!(HookTarget::Kimi.codec(), ConfigCodec::Toml);
        assert_eq!(
            HookTarget::Antigravity.codec(),
            ConfigCodec::NamedBundleJson
        );
        assert_eq!(HookTarget::Codex.codec(), ConfigCodec::NestedJson);
        assert_eq!(HookTarget::Cursor.events().gate, "preToolUse");
        assert_eq!(HookTarget::Antigravity.events().start, "PreInvocation");
    }

    #[test]
    fn target_config_paths_project_and_user() {
        let p = Path::new("C:/proj");
        assert_eq!(
            HookTarget::Claude.config_path(p).unwrap(),
            p.join(".claude").join("settings.json")
        );
        assert_eq!(
            HookTarget::QoderCn.config_path(p).unwrap(),
            p.join(".qoder-cn").join("settings.json")
        );
        assert_eq!(
            HookTarget::Codex.config_path(p).unwrap(),
            p.join(".codex").join("hooks.json")
        );
        assert_eq!(
            HookTarget::Cursor.config_path(p).unwrap(),
            p.join(".cursor").join("hooks.json")
        );
        // User-only vendors resolve under the home dir, independent of the project path.
        assert!(
            HookTarget::Kimi
                .config_path(p)
                .unwrap()
                .ends_with("config.toml")
        );
        assert!(
            HookTarget::Antigravity
                .config_path(p)
                .unwrap()
                .ends_with("hooks.json")
        );
    }

    #[test]
    fn entry_command_bakes_vendor_and_event() {
        assert_eq!(
            entry_command(BASE, HookTarget::Cursor, "preToolUse"),
            format!("{BASE} cursor preToolUse")
        );
        assert_eq!(
            entry_command(BASE, HookTarget::QoderCn, "PreToolUse"),
            format!("{BASE} qoder-cn PreToolUse")
        );
    }
}
