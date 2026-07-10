//! 工作空间管理
//!
//! 管理 Agent 操作的工作目录，确保工具调用不超出工作空间边界

use std::path::{Path, PathBuf};
use std::fs;

/// 获取 Goat 数据目录 (`~/.goat/`)
///
/// 与原版 Python Goat 保持一致，Windows 下为 `%USERPROFILE%\.goat\`，
/// 非 Windows 下为 `$HOME/.goat/`。
pub fn get_data_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| dirs::data_dir().unwrap_or_else(|| PathBuf::from(".")))
        .join(".goat")
}

/// 获取 Goat 配置目录（与数据目录相同：`~/.goat/`）
pub fn get_config_dir() -> PathBuf {
    get_data_dir()
}

/// 获取技能目录 (`~/.goat/skills/`)
pub fn get_skills_dir() -> PathBuf {
    get_data_dir().join("skills")
}

/// 获取日志目录
pub fn get_logs_dir() -> PathBuf {
    get_data_dir().join("logs")
}

/// 确保目录结构存在
pub fn ensure_data_dirs() -> std::io::Result<()> {
    let data_dir = get_data_dir();
    fs::create_dir_all(data_dir.join("skills"))?;
    fs::create_dir_all(data_dir.join("logs"))?;
    fs::create_dir_all(data_dir.join("vectors"))?;
    Ok(())
}

/// 工作空间
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
    /// 项目级 `.goat/` 目录
    goat_dir: PathBuf,
}

impl Workspace {
    /// 创建新的工作空间
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let goat_dir = root.join(".goat");
        Self { root, goat_dir }
    }

    /// 获取工作空间根目录
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 获取 `.goat/` 目录
    pub fn goat_dir(&self) -> &Path {
        &self.goat_dir
    }

    /// 获取 `.goat/skills/` 目录
    pub fn skills_dir(&self) -> PathBuf {
        self.goat_dir.join("skills")
    }

    /// 获取 `.goat/rules/` 目录
    pub fn rules_dir(&self) -> PathBuf {
        self.goat_dir.join("rules")
    }

    /// 检查路径是否在工作空间内
    pub fn contains(&self, path: &Path) -> bool {
        // Canonicalize both paths for accurate comparison
        if let (Ok(ws), Ok(p)) = (self.root.canonicalize(), path.canonicalize()) {
            return p.starts_with(&ws);
        }
        // Fallback: simple prefix check
        path.starts_with(&self.root)
    }

    /// 确保 `.goat/` 布局存在
    pub fn ensure_layout(&self) -> std::io::Result<()> {
        fs::create_dir_all(&self.goat_dir)?;
        fs::create_dir_all(self.skills_dir())?;
        fs::create_dir_all(self.rules_dir())?;

        // 创建 GOAT.md 和 skill 目录结构
        let goat_md = self.goat_dir.join("GOAT.md");
        if !goat_md.exists() {
            fs::write(&goat_md, "# Goat Rules\n\nAdd your coding rules here.\n")?;
        }

        Ok(())
    }
}

/// 解析工作空间路径
///
/// 优先级：
/// 1. 显式指定的路径
/// 2. `.goat/` 所在的祖先目录
/// 3. 当前工作目录
pub fn resolve_workspace(cwd: Option<&Path>) -> Workspace {
    let cwd = cwd.unwrap_or_else(|| Path::new("."));

    // 向上查找包含 `.goat/` 的目录
    let mut current = cwd.to_path_buf();
    loop {
        if current.join(".goat").is_dir() {
            return Workspace::new(current);
        }
        if let Some(parent) = current.parent() {
            current = parent.to_path_buf();
        } else {
            break;
        }
    }

    // Fallback to current directory
    Workspace::new(cwd)
}

/// 从给定目录递归向上查找 workspace root（含 .goat/ 或 AGENTS.md 的目录）
pub fn find_workspace_root(start_dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut current = start_dir.to_path_buf();
    loop {
        if current.join(".goat").is_dir() || current.join("AGENTS.md").is_file() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}
