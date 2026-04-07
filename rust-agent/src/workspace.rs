use std::path::{Component, Path, PathBuf};

use anyhow::{Context, bail};

use crate::AgentResult;

pub fn resolve_workspace_path(root: &Path, input: &str) -> AgentResult<PathBuf> {
    let base = if root.exists() {
        root.canonicalize()
            .with_context(|| format!("Failed to resolve workspace root: {}", root.display()))?
    } else if root.is_absolute() {
        normalize_path(root.to_path_buf())
    } else {
        normalize_path(
            std::env::current_dir()
                .context("Failed to determine current directory")?
                .join(root),
        )
    };
    let joined = if Path::new(input).is_absolute() {
        normalize_path(PathBuf::from(input))
    } else {
        normalize_path(base.join(input))
    };

    if !joined.starts_with(&base) {
        bail!("Path escapes workspace: {input}");
    }

    Ok(joined)
}

fn normalize_path(path: PathBuf) -> PathBuf {
    let mut normalized = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }

    normalized
}
