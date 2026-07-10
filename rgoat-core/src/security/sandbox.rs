//! 沙箱抽象层
//!
//! 平台特定的沙箱实现：
//! - Windows: Job Objects
//! - Linux: Namespaces (seccomp)
//! - macOS: sandbox-exec

use std::path::PathBuf;
use std::sync::Mutex;

/// 沙箱级别
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxLevel {
    /// 只读 — 不能修改文件
    ReadOnly,
    /// 工作空间内写入
    WorkspaceWrite,
    /// 危险：完全访问（默认阻止）
    DangerFullAccess,
}

/// 沙箱 trait
pub trait Sandbox: Send + Sync {
    /// Initialize sandbox for current process/thread
    fn init(&self, level: SandboxLevel, workspace: &PathBuf) -> Result<(), SandboxError>;

    /// Check if a path is within sandbox
    fn is_path_allowed(&self, path: &str) -> bool;

    /// Get sandbox level
    fn level(&self) -> SandboxLevel;
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Sandbox initialization failed: {0}")]
    InitFailed(String),
    #[error("Path not allowed: {0}")]
    PathNotAllowed(String),
    #[error("Platform not supported")]
    UnsupportedPlatform,
}

// ============================================================================
// Windows Job Object 沙箱
// ============================================================================

#[cfg(windows)]
pub mod windows {
    use super::*;
    // AsRawHandle is available but not currently needed in this implementation.
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    pub struct WindowsSandbox {
        job_handle: Mutex<Option<HANDLE>>,
        level: Mutex<SandboxLevel>,
        workspace: Mutex<PathBuf>,
    }

    impl WindowsSandbox {
        pub fn new() -> Self {
            Self {
                job_handle: Mutex::new(None),
                level: Mutex::new(SandboxLevel::ReadOnly),
                workspace: Mutex::new(PathBuf::new()),
            }
        }
    }

    impl Sandbox for WindowsSandbox {
        fn init(&self, level: SandboxLevel, workspace: &PathBuf) -> Result<(), SandboxError> {
            *self.level.lock().unwrap() = level;
            *self.workspace.lock().unwrap() = workspace.clone();

            if level == SandboxLevel::DangerFullAccess {
                return Err(SandboxError::InitFailed(
                    "DangerFullAccess sandbox level is not permitted".to_string(),
                ));
            }

            unsafe {
                let job_name = format!(
                    "GoatSandbox_{}\0",
                    std::process::id()
                );
                let job_name_wide: Vec<u16> = job_name.encode_utf16().collect();

                let job = CreateJobObjectW(
                    std::ptr::null(),
                    job_name_wide.as_ptr(),
                );

                if job.is_null() {
                    return Err(SandboxError::InitFailed(
                        "Failed to create Job Object".to_string(),
                    ));
                }

                // Set job limits
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags =
                    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
                    | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;

                let result = SetInformationJobObject(
                    job,
                    JobObjectExtendedLimitInformation,
                    &limits as *const _ as *const _,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );

                if result == 0 {
                    CloseHandle(job);
                    return Err(SandboxError::InitFailed(
                        "Failed to set Job Object limits".to_string(),
                    ));
                }

                // Assign current process to job
                let current_process = GetCurrentProcess();
                let result = AssignProcessToJobObject(job, current_process);

                if result == 0 {
                    CloseHandle(job);
                    return Err(SandboxError::InitFailed(
                        "Failed to assign process to Job Object".to_string(),
                    ));
                }

                *self.job_handle.lock().unwrap() = Some(job);
            }

            Ok(())
        }

        fn is_path_allowed(&self, path: &str) -> bool {
            let workspace = self.workspace.lock().unwrap();

            // Workspace must be initialized
            if workspace.as_os_str().is_empty() {
                return false;
            }

            // Resolve path: if relative, join with workspace; then canonicalize
            let p = std::path::Path::new(path);
            let resolved = if p.is_relative() {
                match std::fs::canonicalize(workspace.join(p)) {
                    Ok(r) => r,
                    Err(_) => return false,
                }
            } else {
                match std::fs::canonicalize(p) {
                    Ok(r) => r,
                    Err(_) => return false,
                }
            };

            // Canonicalize workspace for prefix matching
            let workspace_canonical = match std::fs::canonicalize(workspace.as_path()) {
                Ok(w) => w,
                Err(_) => return false,
            };

            // Prefix check: must be under workspace
            if !resolved.starts_with(&workspace_canonical) {
                return false;
            }

            let resolved_lower = resolved.to_string_lossy().to_lowercase();

            // Windows system blacklist (case-insensitive)
            if resolved_lower.starts_with("c:\\windows\\system32")
                || resolved_lower == "c:\\windows"
                || resolved_lower.starts_with("c:\\windows\\")
            {
                return false;
            }

            // Common protected paths (~/.ssh, ~/.gnupg)
            if let Some(home) = std::env::var("USERPROFILE")
                .ok()
                .or_else(|| std::env::var("HOME").ok())
            {
                for protected in &[".ssh", ".gnupg"] {
                    let protected_path = std::path::Path::new(&home).join(protected);
                    let protected_lower = protected_path.to_string_lossy().to_lowercase();
                    if resolved_lower.starts_with(&protected_lower) {
                        return false;
                    }
                }
            }

            // Project protected directories (check any path component)
            let protected_dirs = [".git", "node_modules", "target"];
            for component in resolved.components() {
                if let Some(name) = component.as_os_str().to_str() {
                    if protected_dirs.contains(&name) {
                        return false;
                    }
                }
            }

            true
        }

        fn level(&self) -> SandboxLevel {
            *self.level.lock().unwrap()
        }
    }

    impl Drop for WindowsSandbox {
        fn drop(&mut self) {
            if let Ok(guard) = self.job_handle.lock() {
                if let Some(handle) = *guard {
                    unsafe { CloseHandle(handle); }
                }
            }
        }
    }

    // SAFETY: HANDLE is an opaque pointer used only in Win32 APIs,
    // not shared across threads directly. The sandbox is immutable after init.
    unsafe impl Send for WindowsSandbox {}
    unsafe impl Sync for WindowsSandbox {}

    impl Default for WindowsSandbox {
        fn default() -> Self {
            Self::new()
        }
    }
}

// ============================================================================
// 工厂
// ============================================================================

/// Create platform-appropriate sandbox
pub fn create_sandbox() -> Box<dyn Sandbox> {
    #[cfg(windows)]
    {
        Box::new(windows::WindowsSandbox::new())
    }
    #[cfg(not(windows))]
    {
        Box::new(NoopSandbox::new())
    }
}

/// No-op sandbox for unsupported platforms
pub struct NoopSandbox {
    level: SandboxLevel,
    workspace: Mutex<PathBuf>,
}

impl NoopSandbox {
    pub fn new() -> Self {
        Self {
            level: SandboxLevel::ReadOnly,
            workspace: Mutex::new(PathBuf::new()),
        }
    }
}

impl Sandbox for NoopSandbox {
    fn init(&self, level: SandboxLevel, workspace: &PathBuf) -> Result<(), SandboxError> {
        // Noop sandbox level is immutable after creation; store workspace for path checks.
        let _ = level;
        *self.workspace.lock().unwrap() = workspace.clone();
        Ok(())
    }

    fn is_path_allowed(&self, path: &str) -> bool {
        let workspace = self.workspace.lock().unwrap();

        // Workspace must be initialized
        if workspace.as_os_str().is_empty() {
            return false;
        }

        // Resolve path: if relative, join with workspace; then canonicalize
        let p = std::path::Path::new(path);
        let resolved = if p.is_relative() {
            match std::fs::canonicalize(workspace.join(p)) {
                Ok(r) => r,
                Err(_) => return false,
            }
        } else {
            match std::fs::canonicalize(p) {
                Ok(r) => r,
                Err(_) => return false,
            }
        };

        // Canonicalize workspace for prefix matching
        let workspace_canonical = match std::fs::canonicalize(workspace.as_path()) {
            Ok(w) => w,
            Err(_) => return false,
        };

        // Prefix check: must be under workspace
        if !resolved.starts_with(&workspace_canonical) {
            return false;
        }

        // Common protected paths (~/.ssh, ~/.gnupg)
        if let Some(home) = std::env::var("HOME").ok() {
            for protected in &[".ssh", ".gnupg"] {
                let protected_path = std::path::Path::new(&home).join(protected);
                if resolved.starts_with(&protected_path) {
                    return false;
                }
            }
        }

        // Project protected directories (check any path component)
        let protected_dirs = [".git", "node_modules", "target"];
        for component in resolved.components() {
            if let Some(name) = component.as_os_str().to_str() {
                if protected_dirs.contains(&name) {
                    return false;
                }
            }
        }

        true
    }

    fn level(&self) -> SandboxLevel {
        self.level
    }
}
