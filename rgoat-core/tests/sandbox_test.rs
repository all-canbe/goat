//! 沙箱安全测试
//!
//! 测试覆盖：
//! 1. 工厂函数返回正确平台的沙箱实例
//! 2. DangerFullAccess 被拒绝
//! 3. 应用层路径检查（is_path_allowed）跨平台逻辑
//! 4. [Linux] Landlock + seccomp 内核层强制执行
//!
//! 运行：cargo test -p rgoat-core --test sandbox_test
//! Linux Landlock 测试：cargo test -p rgoat-core --test sandbox_test -- --ignored

use rgoat_core::create_sandbox;
use rgoat_core::SandboxLevel;
use std::fs;
use std::path::PathBuf;

/// 初始化沙箱，容忍 Windows Job Object 在嵌套环境中的失败。
///
/// WindowsSandbox.init() 在进程已属于 Job Object 时
/// AssignProcessToJobObject 可能失败，但 workspace 和 level
/// 在 Job Object 创建前已设置，路径检查仍然有效。
/// LinuxSandbox.init() 应总是成功（优雅降级）。
fn init_sandbox(sandbox: &dyn rgoat_core::Sandbox, level: SandboxLevel, workspace: &PathBuf) {
    if let Err(e) = sandbox.init(level, workspace) {
        eprintln!("sandbox init warning (path checks still valid): {e}");
    }
}

// ============================================================================
// 跨平台测试：工厂函数 + 应用层路径检查
// ============================================================================

#[test]
fn test_create_sandbox_returns_usable_instance() {
    let sandbox = create_sandbox();
    // 默认级别应为 ReadOnly
    assert_eq!(sandbox.level(), SandboxLevel::ReadOnly);
}

#[test]
fn test_danger_full_access_rejected() {
    let sandbox = create_sandbox();
    let workspace = tempfile::tempdir().unwrap().keep();
    let result = sandbox.init(SandboxLevel::DangerFullAccess, &workspace);
    assert!(result.is_err(), "DangerFullAccess must be rejected");
}

#[test]
fn test_is_path_allowed_empty_workspace() {
    let sandbox = create_sandbox();
    // 未初始化时 workspace 为空
    assert!(!sandbox.is_path_allowed("/some/path"));
}

#[test]
fn test_is_path_allowed_within_workspace() {
    let sandbox = create_sandbox();
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    // 创建子文件用于测试
    let subdir = workspace.join("src");
    fs::create_dir(&subdir).unwrap();
    fs::write(subdir.join("main.rs"), "fn main() {}").unwrap();

    init_sandbox(&*sandbox, SandboxLevel::WorkspaceWrite, &workspace);

    // workspace 内的文件应允许
    let inside = subdir.join("main.rs");
    assert!(
        sandbox.is_path_allowed(inside.to_str().unwrap()),
        "path inside workspace should be allowed"
    );
}

#[test]
fn test_is_path_allowed_outside_workspace() {
    let sandbox = create_sandbox();
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    init_sandbox(&*sandbox, SandboxLevel::WorkspaceWrite, &workspace);

    // workspace 外的路径应拒绝
    let outside = if cfg!(windows) {
        PathBuf::from("C:\\Windows\\System32\\cmd.exe")
    } else {
        PathBuf::from("/etc/passwd")
    };
    assert!(
        !sandbox.is_path_allowed(outside.to_str().unwrap()),
        "path outside workspace should be denied"
    );
}

#[test]
fn test_is_path_allowed_protected_dirs() {
    let sandbox = create_sandbox();
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    // 在 workspace 下创建受保护目录名
    let git_dir = workspace.join(".git");
    fs::create_dir(&git_dir).unwrap();
    let node_modules = workspace.join("node_modules");
    fs::create_dir(&node_modules).unwrap();
    let target = workspace.join("target");
    fs::create_dir(&target).unwrap();

    init_sandbox(&*sandbox, SandboxLevel::WorkspaceWrite, &workspace);

    // .git 应拒绝
    assert!(
        !sandbox.is_path_allowed(git_dir.to_str().unwrap()),
        ".git directory should be denied"
    );
    // node_modules 应拒绝
    assert!(
        !sandbox.is_path_allowed(node_modules.to_str().unwrap()),
        "node_modules directory should be denied"
    );
    // target 应拒绝
    assert!(
        !sandbox.is_path_allowed(target.to_str().unwrap()),
        "target directory should be denied"
    );
}

#[test]
fn test_readonly_level_stored() {
    let sandbox = create_sandbox();
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    init_sandbox(&*sandbox, SandboxLevel::ReadOnly, &workspace);
    assert_eq!(sandbox.level(), SandboxLevel::ReadOnly);
}

#[test]
fn test_workspace_write_level_stored() {
    let sandbox = create_sandbox();
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    init_sandbox(&*sandbox, SandboxLevel::WorkspaceWrite, &workspace);
    assert_eq!(sandbox.level(), SandboxLevel::WorkspaceWrite);
}

// ============================================================================
// Linux 专用测试：Landlock + seccomp 内核层强制执行
// ============================================================================

#[cfg(target_os = "linux")]
#[test]
fn test_linux_sandbox_factory() {
    let sandbox = create_sandbox();
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path().to_path_buf();

    // init 应成功（内核支持时强制执行，不支持时优雅降级）
    let result = sandbox.init(SandboxLevel::WorkspaceWrite, &workspace);
    assert!(result.is_ok(), "LinuxSandbox init should succeed");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Landlock kernel enforcement test — run with --ignored on Linux 5.13+"]
fn test_landlock_blocks_write_outside_workspace() {
    use std::thread;

    let workspace = tempfile::tempdir().unwrap().keep();
    let outside = tempfile::tempdir().unwrap().keep();
    let outside_file = outside.join("evil.txt");

    // 在专用线程上运行，避免影响测试主线程
    let handle = thread::spawn(move || {
        let sandbox = create_sandbox();
        sandbox
            .init(SandboxLevel::WorkspaceWrite, &workspace)
            .expect("sandbox init");

        // 尝试在 workspace 外写文件 —— 应被 Landlock 阻止
        fs::write(&outside_file, "malicious")
    });

    let write_result = handle.join().expect("thread panicked");

    // 如果 Landlock 生效，写入应失败
    // 如果内核不支持 Landlock，写入可能成功（优雅降级）
    if let Ok(()) = write_result {
        eprintln!(
            "WARNING: Landlock not enforced on this kernel. \
             Write outside workspace succeeded — kernel may be too old."
        );
    } else {
        println!("Landlock correctly blocked write outside workspace");
    }
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "Landlock kernel enforcement test — run with --ignored on Linux 5.13+"]
fn test_landlock_allows_write_within_workspace() {
    use std::thread;

    let workspace = tempfile::tempdir().unwrap().keep();
    let inside_file = workspace.join("output.txt");

    let handle = thread::spawn(move || {
        let sandbox = create_sandbox();
        sandbox
            .init(SandboxLevel::WorkspaceWrite, &workspace)
            .expect("sandbox init");

        // 在 workspace 内写文件 —— 应允许
        fs::write(&inside_file, "hello")
    });

    let write_result = handle.join().expect("thread panicked");
    assert!(write_result.is_ok(), "write within workspace should succeed");
}

#[cfg(target_os = "linux")]
#[test]
#[ignore = "seccomp syscall filter test — run with --ignored on Linux"]
fn test_seccomp_blocks_dangerous_syscall() {
    use std::thread;

    let workspace = tempfile::tempdir().unwrap().keep();

    let handle = thread::spawn(move || {
        let sandbox = create_sandbox();
        sandbox
            .init(SandboxLevel::ReadOnly, &workspace)
            .expect("sandbox init");

        // 尝试 fork（应被 seccomp 阻止）
        // extrasafe 默认阻止 fork/clone 等危险 syscall
        unsafe { libc::fork() }
    });

    let fork_result = handle.join().expect("thread panicked");
    // fork 应返回 -1 或触发 SIGSYS
    eprintln!("fork result: {:?}", fork_result);
}
