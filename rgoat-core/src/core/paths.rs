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
