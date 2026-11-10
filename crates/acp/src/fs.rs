use std::path::{Path, PathBuf};

use agent_control_core::Store;
use agent_control_core::model::EvidenceKind;
use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub fn resolve_safe_path(project_root: &Path, requested: &Path) -> Result<PathBuf> {
    let root = project_root
        .canonicalize()
        .unwrap_or_else(|_| project_root.to_path_buf());
    let joined = if requested.is_absolute() {
        requested.to_path_buf()
    } else {
        root.join(requested)
    };
    let normalized = normalize_lexically(&joined)?;
    let root_normalized = normalize_lexically(&root)?;
    if !normalized.starts_with(&root_normalized) {
        bail!("path '{}' escapes project root", requested.display());
    }
    Ok(normalized)
}

fn normalize_lexically(path: &Path) -> Result<PathBuf> {
    let mut parts: Vec<std::ffi::OsString> = Vec::new();
    for comp in path.components() {
        match comp {
            std::path::Component::ParentDir => {
                parts.pop();
            }
            std::path::Component::CurDir => {}
            other => parts.push(other.as_os_str().to_os_string()),
        }
    }
    let mut result = PathBuf::new();
    for part in parts {
        result.push(part);
    }
    Ok(result)
}

pub fn read_file(project_root: &Path, requested: &Path) -> Result<String> {
    let path = resolve_safe_path(project_root, requested)?;
    std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}

pub fn write_file_with_evidence(
    store: &Store,
    session_id: Uuid,
    agent_id: &str,
    project_root: &Path,
    requested: &Path,
    content: &str,
) -> Result<()> {
    let path = resolve_safe_path(project_root, requested)?;
    let before_hash = if path.exists() {
        Some(hash_file(&path)?)
    } else {
        None
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
    let after_hash = hash_file(&path)?;
    store.record_evidence(
        session_id,
        EvidenceKind::FileChange,
        agent_id,
        serde_json::json!({
            "path": path.to_string_lossy(),
            "before_sha256": before_hash,
            "after_sha256": after_hash,
            "bytes_written": content.len(),
        }),
    )?;
    Ok(())
}

pub fn hash_file(path: &Path) -> Result<String> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    Ok(hex::encode(hasher.finalize()))
}
