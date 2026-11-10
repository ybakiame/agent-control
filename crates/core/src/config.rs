use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub const APP_DIR_NAME: &str = ".agent-control";
pub const PROJECT_DIR_NAME: &str = ".agent-control";
pub const DB_FILE_NAME: &str = "control.db";

#[derive(Debug, Clone)]
pub struct DataLayout {
    pub root: PathBuf,
}

impl DataLayout {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn default_layout() -> Result<Self> {
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
        self.root.join(DB_FILE_NAME)
    }

    pub fn bundles_dir(&self) -> PathBuf {
        self.root.join("bundles")
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        for dir in [self.user_rules_dir(), self.bundles_dir()] {
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
}
