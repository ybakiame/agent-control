use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const APP_DIR_NAME: &str = ".agent-control";
pub const PROJECT_DIR_NAME: &str = ".agent-control";
pub const DB_FILE_NAME: &str = "control.db";
/// Marker file next to the executable that switches the store to portable mode.
pub const PORTABLE_MARKER: &str = "portable.marker";
/// Environment variable that forces portable mode (alternative to the marker file).
pub const PORTABLE_ENV: &str = "AGENT_CONTROL_PORTABLE";
/// Per-project scaffold marker recording the template version and the content hash of each
/// generated file, so upgrades can refresh untouched scaffolds without clobbering user edits.
pub const SCAFFOLD_MARKER: &str = ".scaffold.json";
/// User-editable application config (TOML) at the data root.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// Legacy JSON prefs, converted to `config.toml` on first load and then removed.
const LEGACY_PREFS_FILE: &str = "ui.json";
/// Default loopback port for the MCP daemon; distinct ports isolate test vs production.
pub const DEFAULT_MCP_PORT: u16 = 9870;

/// Application preferences persisted as TOML beside the store (locale + MCP port).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// UI locale code (`en` | `zh`).
    pub locale: String,
    /// Loopback port the MCP daemon binds.
    pub mcp_port: u16,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            locale: "en".to_string(),
            mcp_port: DEFAULT_MCP_PORT,
        }
    }
}

impl AppConfig {
    /// Read `config.toml`; on first run convert a legacy `ui.json` (locale only) and delete it.
    pub fn load(layout: &DataLayout) -> Self {
        if let Ok(text) = std::fs::read_to_string(layout.config_file())
            && let Ok(cfg) = toml::from_str::<Self>(&text)
        {
            return cfg;
        }
        let mut cfg = Self::default();
        let legacy = layout.root.join(LEGACY_PREFS_FILE);
        if let Ok(text) = std::fs::read_to_string(&legacy) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
                && let Some(locale) = value.get("locale").and_then(|v| v.as_str())
            {
                cfg.locale = locale.to_string();
            }
            if cfg.save(layout).is_ok() {
                let _ = std::fs::remove_file(&legacy);
            }
        }
        cfg
    }

    pub fn save(&self, layout: &DataLayout) -> Result<()> {
        let text = toml::to_string_pretty(self).context("serializing config")?;
        std::fs::write(layout.config_file(), text)
            .with_context(|| format!("writing {}", layout.config_file().display()))
    }
}

#[derive(Debug, Clone)]
pub struct DataLayout {
    pub root: PathBuf,
    pub portable: bool,
}

impl DataLayout {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            portable: false,
        }
    }

    /// Portable store rooted at an explicit directory; the database lives under `<root>/db`.
    pub fn portable_with_root(root: PathBuf) -> Self {
        Self {
            root,
            portable: true,
        }
    }

    /// Portable store beside the running executable: `<exe dir>/data`.
    pub fn portable_layout() -> Result<Self> {
        let exe = std::env::current_exe().context("cannot determine executable path")?;
        let dir = exe.parent().context("executable has no parent directory")?;
        Ok(Self::portable_with_root(dir.join("data")))
    }

    /// True when a `portable.marker` file sits next to the executable or `PORTABLE_ENV` is set.
    pub fn portable_requested() -> bool {
        if let Some(v) = std::env::var_os(PORTABLE_ENV) {
            let s = v.to_string_lossy();
            if !s.is_empty() && s != "0" {
                return true;
            }
        }
        std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|d| d.join(PORTABLE_MARKER).exists()))
            .unwrap_or(false)
    }

    pub fn default_layout() -> Result<Self> {
        if Self::portable_requested() {
            return Self::portable_layout();
        }
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(Self::new(home.join(APP_DIR_NAME)))
    }

    pub fn user_rules_dir(&self) -> PathBuf {
        self.root.join("rules").join("user")
    }

    pub fn agents_file(&self) -> PathBuf {
        self.root.join("agents.yaml")
    }

    pub fn config_file(&self) -> PathBuf {
        self.root.join(CONFIG_FILE_NAME)
    }

    pub fn db_path(&self) -> PathBuf {
        if self.portable {
            self.root.join("db").join(DB_FILE_NAME)
        } else {
            self.root.join(DB_FILE_NAME)
        }
    }

    pub fn bundles_dir(&self) -> PathBuf {
        self.root.join("bundles")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        let mut dirs = vec![self.user_rules_dir(), self.bundles_dir(), self.logs_dir()];
        if let Some(parent) = self.db_path().parent() {
            dirs.push(parent.to_path_buf());
        }
        for dir in dirs {
            std::fs::create_dir_all(&dir)
                .with_context(|| format!("creating directory {}", dir.display()))?;
        }
        Ok(())
    }

    pub fn project_config_dir(project_path: &Path) -> PathBuf {
        project_path.join(PROJECT_DIR_NAME)
    }

    pub fn project_rules_dir(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join("rules")
    }

    /// Project-local, git-ignored environment notes (this user × this project × this machine). Kept
    /// outside `rules/` so machine-specific facts are never committed alongside the shared contracts;
    /// a `.gitignore` inside it ignores everything but itself. Delivered only via the MCP/hook briefing.
    pub fn project_environment_dir(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join("local")
    }

    /// Template rule files shipped with the scaffold. Kept outside `rules/` so they are never
    /// loaded as binding contracts; copy one into `rules/` to activate it.
    pub fn project_examples_dir(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join("examples")
    }

    /// Scaffold marker tracking template version + per-file hashes for safe upgrades.
    pub fn project_scaffold_marker(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join(SCAFFOLD_MARKER)
    }

    /// Project-local continuity brief written by `export_handoff`. The managed block in AGENTS.md
    /// points here so any harness inherits the latest handoff by reading a file, without an MCP call.
    pub fn project_handoff_dir(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join("handoff")
    }

    pub fn project_handoff_latest(project_path: &Path) -> PathBuf {
        Self::project_handoff_dir(project_path).join("latest.md")
    }

    /// Native harness entry points the binding set is compiled into (managed-block targets).
    pub fn project_agents_md(project_path: &Path) -> PathBuf {
        project_path.join("AGENTS.md")
    }

    pub fn project_claude_md(project_path: &Path) -> PathBuf {
        project_path.join("CLAUDE.md")
    }

    /// Gemini CLI reads `GEMINI.md` (not `AGENTS.md`); opt-in export target.
    pub fn project_gemini_md(project_path: &Path) -> PathBuf {
        project_path.join("GEMINI.md")
    }

    /// Dedicated Cursor rule file (frontmatter + managed block).
    pub fn project_cursor_rule(project_path: &Path) -> PathBuf {
        project_path
            .join(".cursor")
            .join("rules")
            .join("agent-control.mdc")
    }

    /// User-global Claude Code memory; personal preferences export here so they are not committed
    /// into a project repo.
    pub fn claude_user_md() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".claude").join("CLAUDE.md"))
    }

    /// Project-level harness `settings.json` files the Agent Control hook registers into. These are
    /// the shared (committed) config, not the device-local `settings.local.json` variants.
    pub fn project_claude_settings(project_path: &Path) -> PathBuf {
        project_path.join(".claude").join("settings.json")
    }

    pub fn project_codebuddy_settings(project_path: &Path) -> PathBuf {
        project_path.join(".codebuddy").join("settings.json")
    }

    pub fn project_workbuddy_settings(project_path: &Path) -> PathBuf {
        project_path.join(".workbuddy").join("settings.json")
    }

    pub fn project_qoder_settings(project_path: &Path) -> PathBuf {
        project_path.join(".qoder").join("settings.json")
    }

    /// User-global harness config dirs, used to detect whether a harness is installed on this machine.
    pub fn claude_user_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".claude"))
    }

    pub fn codebuddy_user_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".codebuddy"))
    }

    pub fn workbuddy_user_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".workbuddy"))
    }

    pub fn qoder_user_dir() -> Result<PathBuf> {
        let home = dirs::home_dir().context("cannot determine home directory")?;
        Ok(home.join(".qoder-cn"))
    }
}
