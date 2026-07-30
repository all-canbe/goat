//! 路径工具 — 常用路径解析和安全检查

use std::path::{Path, PathBuf};

/// 安全解析路径（展开 ~ 并转为绝对路径）
pub fn safe_path(path: &str, workspace: &Path) -> PathBuf {
    let expanded = expand_tilde(path);
    let p = PathBuf::from(&expanded);

    if p.is_absolute() {
        p
    } else {
        workspace.join(&p).canonicalize().unwrap_or_else(|_| workspace.join(&p))
    }
}

/// Resolve a read-tool `path` argument and enforce workspace containment.
///
/// Used by read-only tools (Glob, Grep) so that absolute paths and relative
/// `..` escapes pointing outside the workspace are rejected with a uniform
/// `path_outside_workspace` error. Symlinks are followed via `canonicalize`,
/// so a symlink inside the workspace that points outside is also rejected.
///
/// Non-existent paths: a glob/grep root that does not yet exist must keep the
/// tool's existing "no results" semantics. We therefore canonicalize the
/// nearest existing ancestor, verify *its* containment, and return the
/// requested path with its non-existent tail re-attached — instead of
/// reporting a missing path as an escape.
pub fn resolve_workspace_path(path: &str, workspace: &Path) -> Result<PathBuf, String> {
    let workspace = workspace.canonicalize().map_err(|error| error.to_string())?;
    let candidate = PathBuf::from(expand_tilde(path));
    let candidate = if candidate.is_absolute() {
        candidate
    } else {
        workspace.join(candidate)
    };

    // Fast path: candidate exists — canonicalize (follows symlinks) and check.
    if let Ok(resolved) = candidate.canonicalize() {
        if !resolved.starts_with(&workspace) {
            return Err("path_outside_workspace".to_string());
        }
        return Ok(resolved);
    }

    // Candidate does not exist (e.g. an empty glob root). Walk up to the
    // nearest existing ancestor and verify containment there, then re-attach
    // the missing tail so the caller can produce its own empty-result output.
    let mut ancestor = candidate.clone();
    while !ancestor.exists() {
        if !ancestor.pop() {
            // Reached the filesystem root without an existing ancestor.
            return Err("path_outside_workspace".to_string());
        }
    }
    let resolved_ancestor = ancestor.canonicalize().map_err(|error| error.to_string())?;
    if !resolved_ancestor.starts_with(&workspace) {
        return Err("path_outside_workspace".to_string());
    }
    // `ancestor` is a prefix of `candidate`, so stripping always succeeds.
    let tail = candidate.strip_prefix(&ancestor).unwrap_or_else(|_| Path::new(""));
    Ok(resolved_ancestor.join(tail))
}

/// Expand ~ to home directory
pub fn expand_tilde(path: &str) -> String {
    if path.starts_with('~') {
        if let Some(home) = dirs::home_dir() {
            if path == "~" {
                return home.to_string_lossy().to_string();
            }
            if path.starts_with("~/") {
                return home.join(&path[2..]).to_string_lossy().to_string();
            }
        }
    }
    path.to_string()
}

/// Check if a path is safe to operate on (not a system directory)
pub fn is_safe_path(path: &Path, workspace: &Path) -> bool {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());

    // Must be within workspace
    let ws = workspace.canonicalize().unwrap_or_else(|_| workspace.to_path_buf());
    if !path.starts_with(&ws) {
        return false;
    }

    // Block .git directory modifications (can still read)
    let components: Vec<_> = path.components().collect();
    for comp in &components {
        let s = comp.as_os_str().to_string_lossy();
        if s == ".git" {
            return false;
        }
    }

    true
}

/// Get relative path from workspace root
pub fn relative_path(path: &Path, workspace: &Path) -> Option<String> {
    path.strip_prefix(workspace)
        .ok()
        .map(|p| p.to_string_lossy().to_string())
}
