use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

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

    /// Template rule files shipped with the scaffold. Kept outside `rules/` so they are never
    /// loaded as binding contracts; copy one into `rules/` to activate it.
    pub fn project_examples_dir(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join("examples")
    }

    /// Scaffold marker tracking template version + per-file hashes for safe upgrades.
    pub fn project_scaffold_marker(project_path: &Path) -> PathBuf {
        Self::project_config_dir(project_path).join(SCAFFOLD_MARKER)
    }
}
