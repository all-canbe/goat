//! Tool optimization integration tests.
//!
//! Covers behavior verified by the tool-optimization remediation plan.

use rgoat_core::core::cancellation::CancellationToken;
use rgoat_core::security::approval::ToolCategory;
use rgoat_core::tools::builtin::{git_subcommand_category, GitTool, GlobTool, GrepTool};
use rgoat_core::tools::registry::{Tool, ToolExecutionContext, ToolStreamEvent};
use rgoat_core::tools::shell::ShellTool;
use rgoat_core::tools::web_fetch::{web_fetch_client, SsrfChecker, WebFetchTool};
use rgoat_core::tools::edit_file::EditFileTool;
use rgoat_core::tools::file_mutation_queue::FileMutationQueue;
use rgoat_core::tools::read_file::ReadFileTool;
use rgoat_core::tools::web_search::{SearchResult, WebSearchProvider, WebSearchTool};
use rgoat_core::tools::write_file::WriteFileTool;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

/// Mock provider that always returns a fixed set of results, ignoring
/// `max_results`. Used to verify that `WebSearchTool::execute` caps the
/// final output to `max_results`.
struct FixedSearchProvider {
    results: Vec<SearchResult>,
}

impl FixedSearchProvider {
    fn new(results: Vec<SearchResult>) -> Self {
        Self { results }
    }
}

#[async_trait::async_trait]
impl WebSearchProvider for FixedSearchProvider {
    async fn search(&self, _query: &str, _max_results: usize) -> Result<Vec<SearchResult>, String> {
        Ok(self.results.clone())
    }
}

#[tokio::test]
async fn web_search_caps_results_without_api_key() {
    let provider = Box::new(FixedSearchProvider::new(vec![
        SearchResult { title: "one".into(), url: "https://one.test".into(), snippet: "a".into() },
        SearchResult { title: "two".into(), url: "https://two.test".into(), snippet: "b".into() },
    ]));
    let tool = WebSearchTool::with_provider(provider, None);

    let result = tool.execute(serde_json::json!({
        "query": "rust",
        "max_results": 1,
    })).await;

    assert!(result.success);
    assert!(result.output.contains("one"));
    assert!(!result.output.contains("two"));
}

// ============================================================================
// WebFetch — SSRF redirect & client policy tests (Task 2)
// ============================================================================

/// Spawn a local TCP fixture: a "public" entry listener that responds `302`
/// redirecting to a loopback "private" listener. Returns `(public_url,
/// private_was_hit)`. The private listener records whether it was contacted.
async fn spawn_redirect_fixture() -> (String, Arc<AtomicBool>) {
    let private_hit = Arc::new(AtomicBool::new(false));

    // Private target — must never receive a request.
    let private_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let private_port = private_listener.local_addr().unwrap().port();
    let hit = private_hit.clone();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = private_listener.accept().await {
            hit.store(true, Ordering::SeqCst);
            let body = b"private secret";
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            let _ = sock.write_all(body).await;
        }
    });

    // Public entry — issues a 302 redirect to the loopback private target.
    let public_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let public_port = public_listener.local_addr().unwrap().port();
    let private_location = format!("http://127.0.0.1:{}/private", private_port);
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = public_listener.accept().await {
            drain_request(&mut sock).await;
            let resp = format!(
                "HTTP/1.1 302 Found\r\nLocation: {}\r\nContent-Length: 0\r\n\r\n",
                private_location
            );
            let _ = sock.write_all(resp.as_bytes()).await;
            // Hold the socket so reqwest finishes parsing the 302 headers.
            let mut buf = [0u8; 64];
            let _ = sock.read(&mut buf).await;
        }
    });

    (
        format!("http://127.0.0.1:{}/", public_port),
        private_hit,
    )
}

/// Mock SsrfChecker that allows the first check (the public entry URL) and
/// blocks every subsequent check (the loopback redirect target). Both
/// fixtures bind 127.0.0.1, so the mock distinguishes them by call order:
/// hop 0 = entry (allowed), hop 1 = redirect target (blocked as loopback).
struct MockSsrfChecker {
    calls: AtomicU32,
}

impl MockSsrfChecker {
    fn new() -> Self {
        Self {
            calls: AtomicU32::new(0),
        }
    }
}

#[async_trait::async_trait]
impl SsrfChecker for MockSsrfChecker {
    async fn check(&self, _host: &str) -> Result<(), String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            // Public entry — allowed.
            Ok(())
        } else {
            // Redirect target — blocked as loopback.
            Err("ssrf_blocked: redirect target is loopback".to_string())
        }
    }
}

#[tokio::test]
async fn web_fetch_rejects_redirect_to_loopback() {
    let (public_url, private_hit) = spawn_redirect_fixture().await;
    let tool = WebFetchTool::new_with_ssrf_checker(
        web_fetch_client(),
        None,
        Arc::new(MockSsrfChecker::new()),
    );

    let result = tool
        .execute(serde_json::json!({ "url": public_url }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("ssrf_blocked"));
    assert!(
        !private_hit.load(Ordering::SeqCst),
        "private loopback listener must not be contacted"
    );
}

/// Read and discard the client request line + headers (up to the blank
/// `\r\n\r\n` terminator) so the fixture doesn't reset the connection before
/// reqwest finishes writing its request.
async fn drain_request(sock: &mut tokio::net::TcpStream) {
    let mut buf = [0u8; 1024];
    loop {
        match sock.read(&mut buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if buf[..n].windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
}

#[tokio::test]
async fn web_fetch_client_does_not_follow_redirects() {
    // The dedicated WebFetch client must disable reqwest's auto-redirect so
    // `fetch_url` can validate each hop. A default `Client::new()` would
    // silently follow the 302 and never surface `is_redirection()`.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        if let Ok((mut sock, _)) = listener.accept().await {
            drain_request(&mut sock).await;
            let resp =
                "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/x\r\nContent-Length: 0\r\n\r\n";
            let _ = sock.write_all(resp.as_bytes()).await;
            // Hold the socket so reqwest finishes parsing the 302 headers.
            let mut buf = [0u8; 64];
            let _ = sock.read(&mut buf).await;
        }
    });

    let url = format!("http://127.0.0.1:{}/", port);
    let resp = web_fetch_client().get(&url).send().await.unwrap();

    assert!(
        resp.status().is_redirection(),
        "web_fetch_client must disable auto-redirect, got status {}",
        resp.status()
    );
}

// ============================================================================
// Glob / Grep — workspace containment boundary (Task 3)
// ============================================================================
//
// Read-only tools (Glob, Grep) must reject any `path` argument that resolves
// outside the workspace root, whether supplied as an absolute path or as a
// relative path that escapes via `..`. The error code is
// `path_outside_workspace`; non-existent in-workspace paths must still return
// the tool's existing "no results" semantics (not be misreported as escape).

/// Build a relative `../<outside_name>` path that escapes the workspace.
/// Both tempdirs are siblings under the system temp root, so this resolves
/// to `outside.path()` when joined onto the workspace.
fn escape_relative_to(outside: &tempfile::TempDir) -> String {
    let name = outside.path().file_name().unwrap();
    std::path::Path::new("..")
        .join(name)
        .to_string_lossy()
        .to_string()
}

#[tokio::test]
async fn glob_rejects_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let tool = GlobTool::new(workspace.path().to_path_buf(), None);

    let result = tool
        .execute(serde_json::json!({
            "pattern": "**/*",
            "path": outside.path().to_string_lossy(),
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
}

#[tokio::test]
async fn grep_rejects_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let tool = GrepTool::new(workspace.path().to_path_buf(), None);

    let result = tool
        .execute(serde_json::json!({
            "pattern": "secret",
            "path": outside.path().to_string_lossy(),
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
}

#[tokio::test]
async fn glob_rejects_relative_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let rel = escape_relative_to(&outside);

    let tool = GlobTool::new(workspace.path().to_path_buf(), None);
    let result = tool
        .execute(serde_json::json!({
            "pattern": "**/*",
            "path": rel,
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
}

#[tokio::test]
async fn grep_rejects_relative_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let rel = escape_relative_to(&outside);

    let tool = GrepTool::new(workspace.path().to_path_buf(), None);
    let result = tool
        .execute(serde_json::json!({
            "pattern": "secret",
            "path": rel,
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
}

/// A non-existent path *inside* the workspace must not be misreported as an
/// escape: the tool should walk an empty root and return "No files found".
#[tokio::test]
async fn glob_nonexistent_in_workspace_path_is_not_escape() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = GlobTool::new(workspace.path().to_path_buf(), None);

    let result = tool
        .execute(serde_json::json!({
            "pattern": "**/*",
            "path": workspace.path().join("does_not_exist").to_string_lossy(),
        }))
        .await;

    assert!(result.success, "got error: {:?}", result.error);
    assert_eq!(result.output, "No files found");
}

// ============================================================================
// Grep — match-count limit & context semantics (Task 4)
// ============================================================================
//
// `max_results` caps the number of *matched lines*, not context lines. Context
// lines are emitted but do not consume the budget. `truncated` must be true
// only when an (N+1)th match actually exists, never inferred from output length.

/// Three matches with one line of trailing context each, searched with
/// `max_results: 2`. The first two matches (plus their context) must appear;
/// the third match and its context must not. `match_count` counts only
/// matched lines, and `truncated` must be true with a truncation hint.
#[tokio::test]
async fn grep_caps_match_count_with_context() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("lines.txt");
    std::fs::write(
        &file,
        "alpha match\nctx for one\nfiller line\nbeta match\nctx for two\nfiller line\ngamma match\nctx for three\n",
    )
    .unwrap();

    let tool = GrepTool::new(workspace.path().to_path_buf(), None);
    let result = tool
        .execute(serde_json::json!({
            "pattern": "match",
            "path": file.to_string_lossy(),
            "before_context": 0,
            "after_context": 1,
            "max_results": 2,
        }))
        .await;

    assert!(result.success, "got error: {:?}", result.error);

    let meta = result.metadata.expect("metadata must be present");
    assert_eq!(meta["match_count"], 2, "match_count metadata: {}", meta);
    assert_eq!(meta["truncated"], true, "truncated metadata: {}", meta);

    // First two matches and their de-duplicated context are present.
    assert!(result.output.contains("alpha match"));
    assert!(result.output.contains("ctx for one"));
    assert!(result.output.contains("beta match"));
    assert!(result.output.contains("ctx for two"));

    // Third match and its context must be absent.
    assert!(!result.output.contains("gamma match"));
    assert!(!result.output.contains("ctx for three"));

    // Truncation hint must appear only when the limit was actually hit.
    assert!(result.output.contains("Showing first 2 matches"));
}

/// Exactly two matches with `max_results: 2` must NOT be truncated: there is
/// no third match, so `truncated` is false and no hint is shown. This must be
/// determined by checking for an (N+1)th match, never by output length.
#[tokio::test]
async fn grep_no_truncation_at_exact_boundary() {
    let workspace = tempfile::tempdir().unwrap();
    let file = workspace.path().join("lines.txt");
    std::fs::write(
        &file,
        "alpha match\nfiller one\nbeta match\nfiller two\n",
    )
    .unwrap();

    let tool = GrepTool::new(workspace.path().to_path_buf(), None);
    let result = tool
        .execute(serde_json::json!({
            "pattern": "match",
            "path": file.to_string_lossy(),
            "max_results": 2,
        }))
        .await;

    assert!(result.success, "got error: {:?}", result.error);

    let meta = result.metadata.expect("metadata must be present");
    assert_eq!(meta["match_count"], 2, "match_count metadata: {}", meta);
    assert_eq!(meta["truncated"], false, "truncated metadata: {}", meta);

    assert!(!result.output.contains("Showing first"));
    assert!(result.output.contains("alpha match"));
    assert!(result.output.contains("beta match"));
}

// ============================================================================
// Shell streaming — tool_call_id, total timeout, cancellation, output drain
// (Task 5)
// ============================================================================
//
// `ShellTool::execute_streaming` must: (1) tag stream events with the real
// model-issued tool_call_id (not a hardcoded "shell-streaming"), (2) enforce a
// total deadline measured from spawn (not an idle/output-gap timeout), (3)
// honor the per-call ctx cancellation token in addition to the tool-level
// token, and (4) drain reader tasks fully so trailing stdout/stderr lines are
// never lost to a race with child.wait().

/// Stream events must carry the model-issued tool_call_id passed via the
/// context, never the fabricated "shell-streaming" id.
#[tokio::test]
async fn shell_streaming_uses_real_tool_call_id() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(workspace.path().to_path_buf(), None);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ToolStreamEvent>();
    let ctx = ToolExecutionContext::with_streaming(
        "call_shell_001".to_string(),
        None,
        tx,
    );

    // Stream updates are throttled at 100ms; run a command that keeps emitting
    // past 100ms so at least one event is flushed to the channel. On Windows
    // a large echo loop reliably exceeds the throttle window without depending
    // on `ping` (which can hang when spawned via cmd /C with piped stdout).
    #[cfg(windows)]
    let command = "for /L %i in (1,1,50000) do @echo line%i";
    #[cfg(not(windows))]
    let command = "i=0; while [ $i -lt 4 ]; do echo line$i; sleep 0.04; i=$((i+1)); done";

    let result = tool
        .execute_ctx(
            serde_json::json!({ "command": command, "timeout_ms": 5000 }),
            &ctx,
        )
        .await;
    assert!(result.success, "got error: {:?}", result.error);

    let mut events = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        events.push(ev);
    }
    assert!(!events.is_empty(), "expected at least one stream event");
    for ev in &events {
        assert_eq!(
            ev.tool_call_id, "call_shell_001",
            "stream event must carry the real tool_call_id"
        );
        assert_ne!(
            ev.tool_call_id, "shell-streaming",
            "must not use the hardcoded fake id"
        );
    }
}

/// The timeout is a total deadline from spawn, not an idle/output-gap timeout.
/// Continuous output (gaps < timeout_ms) must NOT prevent the deadline from
/// firing; an idle-timeout implementation would let the command run to
/// completion and return success.
#[tokio::test]
async fn shell_streaming_total_timeout_not_idle() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(workspace.path().to_path_buf(), None);
    let ctx = ToolExecutionContext::new(None);

    // Continuous output (line gaps well under 100ms) that runs past the 100ms
    // deadline. An idle/output-gap timeout never fires here; only a total
    // deadline from spawn can fire while output is still flowing.
    #[cfg(windows)]
    let command = "for /L %i in (1,1,5000) do @echo %i";
    #[cfg(not(windows))]
    let command = "i=0; while [ $i -lt 20 ]; do echo $i; sleep 0.04; i=$((i+1)); done";

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        tool.execute_ctx(
            serde_json::json!({ "command": command, "timeout_ms": 100 }),
            &ctx,
        ),
    )
    .await
    .expect("execute_ctx must not hang beyond 2s");

    assert!(!result.success, "expected timeout, got success");
    assert_eq!(result.error.as_deref(), Some("timeout"));
}

/// The per-call ctx.cancellation token must be honored in addition to the
/// tool-level token. Here the tool has no token (None); only ctx can cancel.
#[tokio::test]
async fn shell_streaming_respects_ctx_cancellation() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(workspace.path().to_path_buf(), None);

    let token = CancellationToken::new();
    let ctx = ToolExecutionContext::new(Some(token.clone()));

    // ~1s runtime, no stdout — short enough to stay under the 2s guard, long
    // enough that a cancellation at 150ms clearly precedes natural completion.
    #[cfg(windows)]
    let command = "ping -n 2 127.0.0.1 >nul";
    #[cfg(not(windows))]
    let command = "sleep 1";

    let cancel_token = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(150)).await;
        cancel_token.cancel();
    });

    let result = tokio::time::timeout(
        Duration::from_secs(2),
        tool.execute_ctx(
            serde_json::json!({ "command": command, "timeout_ms": 5000 }),
            &ctx,
        ),
    )
    .await
    .expect("execute_ctx must not hang beyond 2s");

    assert!(!result.success, "expected cancelled, got success");
    assert_eq!(result.error.as_deref(), Some("cancelled"));
}

/// After child.wait() returns, reader tasks may still be flushing the trailing
/// line into the channel. A non-blocking try_recv drain races and can drop it;
/// awaiting the reader tasks then recv().await drains deterministically.
#[tokio::test]
async fn shell_streaming_drains_tail_output() {
    let workspace = tempfile::tempdir().unwrap();
    let tool = ShellTool::new(workspace.path().to_path_buf(), None);
    let ctx = ToolExecutionContext::new(None);

    // "first" -> stdout, "last" -> stderr.
    #[cfg(windows)]
    let command = "echo first & echo last 1>&2";
    #[cfg(not(windows))]
    let command = r"printf 'first\n'; printf 'last\n' >&2";

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        tool.execute_ctx(
            serde_json::json!({ "command": command, "timeout_ms": 5000 }),
            &ctx,
        ),
    )
    .await
    .expect("execute_ctx must not hang");

    assert!(result.success, "got error: {:?}", result.error);
    assert!(
        result.output.contains("first"),
        "stdout line must be drained: {}",
        result.output
    );
    assert!(
        result.output.contains("last"),
        "stderr line must be drained: {}",
        result.output
    );
}

// ============================================================================
// Git — shlex argument preservation & destructive classification (Task 7)
// ============================================================================
//
// GitTool must parse `command` with shlex so quoted arguments containing
// spaces survive intact (`commit -m "a b"` → message `a b`, not `"a`/`b"`),
// and destructive subcommands (`reset --hard`) must classify as
// `Destructive` so they route through approval rather than being silently
// treated as read-only. All tests run inside a TempDir; the caller's real
// workspace repository is never read or modified.

/// Run `git <args>` inside `dir`, panicking with captured stderr on failure.
/// Used only for test fixture setup (init/config/add/commit), never for the
/// GitTool-under-test invocation.
fn git_run(dir: &std::path::Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("git CLI must be available on PATH for GitTool tests");
    if !output.status.success() {
        panic!(
            "git {:?} failed with status {:?}: {}",
            args,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Create a temporary git repository with a configured identity and one
/// initial commit. The returned TempDir must be kept alive for the test
/// duration; dropping it cleans up the repository.
fn setup_temp_git_repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git_run(dir.path(), &["init"]);
    git_run(dir.path(), &["config", "user.name", "Test User"]);
    git_run(dir.path(), &["config", "user.email", "test@example.com"]);
    std::fs::write(dir.path().join("README.md"), "initial\n").unwrap();
    git_run(dir.path(), &["add", "README.md"]);
    git_run(dir.path(), &["commit", "-m", "initial"]);
    dir
}

/// `git commit -m "a b"` must record the spaced message verbatim, and
/// `git log -1 --format=%s` must return it. With the old
/// `split_whitespace()` tokenizer the message would be fragmented into
/// `"a`/`b"` (with stray quotes), corrupting the commit subject.
#[tokio::test]
async fn git_tool_commit_preserves_args() {
    let dir = setup_temp_git_repo();
    // Stage a new file so the GitTool commit has something to record beyond
    // the initial commit created by the fixture.
    std::fs::write(dir.path().join("second.txt"), "2\n").unwrap();
    git_run(dir.path(), &["add", "second.txt"]);

    let tool = GitTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(serde_json::json!({ "command": "commit -m \"a b\"" }))
        .await;
    assert!(
        result.success,
        "commit failed: {:?} / {}",
        result.error,
        result.output
    );

    let log = tool
        .execute(serde_json::json!({ "command": "log -1 --format=%s" }))
        .await;
    assert!(log.success, "log failed: {:?}", log.error);
    assert_eq!(
        log.output.trim(),
        "a b",
        "commit subject with spaces must be preserved end-to-end"
    );
}

/// `git log --grep "a b"` must pass the spaced pattern as a single argument
/// (shlex). A `split_whitespace()` tokenizer would split it into `"a`/`b"`,
/// so the search would either error or fail to match the `a b` commit.
#[tokio::test]
async fn git_tool_grep_preserves_quoted_args() {
    let dir = setup_temp_git_repo();
    std::fs::write(dir.path().join("feature.txt"), "f\n").unwrap();
    git_run(dir.path(), &["add", "feature.txt"]);
    git_run(dir.path(), &["commit", "-m", "a b"]);

    let tool = GitTool::new(dir.path().to_path_buf());
    let result = tool
        .execute(serde_json::json!({ "command": "log --grep \"a b\" --format=%s" }))
        .await;
    assert!(result.success, "git log --grep failed: {:?}", result.error);
    assert!(
        result.output.trim().contains("a b"),
        "spaced --grep pattern must match the 'a b' commit, got: {}",
        result.output
    );
}

/// `reset --hard HEAD` must classify as `Destructive` so it routes through
/// approval, while read-only subcommands stay `Read`. The category function
/// is the approval front-end used by the agent loop before GitTool runs.
#[test]
fn git_tool_reset_categorized_destructive() {
    assert_eq!(
        git_subcommand_category("reset --hard HEAD"),
        ToolCategory::Destructive,
        "reset --hard must be destructive"
    );
    // Sanity: a read-only subcommand stays Read.
    assert_eq!(
        git_subcommand_category("log --oneline"),
        ToolCategory::Read,
        "log must remain read-only"
    );
}

// ============================================================================
// Read / Write / Edit — workspace containment boundary (Task 8 follow-up)
// ============================================================================
//
// The remediation plan's global constraint requires Glob, Grep, Read, Write,
// and Edit to all enforce workspace containment. Task 3 migrated Glob/Grep to
// `resolve_workspace_path`; Read/Write/Edit still used `safe_path`, which
// accepts any absolute path without a boundary check. These tests pin the
// gap: an absolute path pointing outside the workspace must be rejected with
// `path_outside_workspace` (matching Glob/Grep), while in-workspace relative
// paths must continue to work.

/// Read must reject an absolute path that resolves outside the workspace.
/// With `safe_path` the file would be read silently; `resolve_workspace_path`
/// returns `path_outside_workspace` instead.
#[tokio::test]
async fn read_rejects_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "secret").unwrap();
    let tool = ReadFileTool::new(workspace.path().to_path_buf(), None);

    let result = tool
        .execute(serde_json::json!({
            "file_path": outside.path().join("secret.txt").to_string_lossy().to_string(),
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
}

/// Write must reject an absolute path outside the workspace *before* enqueueing
/// the mutation, so the outside file is never created.
#[tokio::test]
async fn write_rejects_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let mutation_queue = Arc::new(FileMutationQueue::new());
    let tool = WriteFileTool::new(
        workspace.path().to_path_buf(),
        None,
        mutation_queue,
    );

    let result = tool
        .execute(serde_json::json!({
            "file_path": outside.path().join("evil.txt").to_string_lossy().to_string(),
            "content": "pwned",
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
    // Confirm the outside file was never created.
    assert!(!outside.path().join("evil.txt").exists());
}

/// Edit must reject an absolute path outside the workspace *before* enqueueing,
/// so the outside file is left untouched.
#[tokio::test]
async fn edit_rejects_path_outside_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(outside.path().join("target.txt"), "old").unwrap();
    let mutation_queue = Arc::new(FileMutationQueue::new());
    let tool = EditFileTool::new(
        workspace.path().to_path_buf(),
        None,
        mutation_queue,
    );

    let result = tool
        .execute(serde_json::json!({
            "file_path": outside.path().join("target.txt").to_string_lossy().to_string(),
            "old_string": "old",
            "new_string": "new",
        }))
        .await;

    assert!(!result.success);
    assert_eq!(result.error.as_deref(), Some("path_outside_workspace"));
    // Confirm the outside file was not modified.
    assert_eq!(
        std::fs::read_to_string(outside.path().join("target.txt")).unwrap(),
        "old"
    );
}

/// A relative path inside the workspace must still read successfully — the
/// boundary check must not break the existing in-workspace read semantics.
#[tokio::test]
async fn read_allows_workspace_relative_path() {
    let workspace = tempfile::tempdir().unwrap();
    std::fs::write(workspace.path().join("note.md"), "hello").unwrap();
    let tool = ReadFileTool::new(workspace.path().to_path_buf(), None);

    let result = tool
        .execute(serde_json::json!({
            "file_path": "note.md",
        }))
        .await;

    assert!(result.success);
    assert!(result.output.contains("hello"));
}

/// Writing a new file at a relative path inside the workspace (including a
/// not-yet-existing parent directory) must still succeed — the boundary check
/// uses the nearest existing ancestor and re-attaches the missing tail.
#[tokio::test]
async fn write_allows_new_file_in_workspace() {
    let workspace = tempfile::tempdir().unwrap();
    let mutation_queue = Arc::new(FileMutationQueue::new());
    let tool = WriteFileTool::new(
        workspace.path().to_path_buf(),
        None,
        mutation_queue,
    );

    let result = tool
        .execute(serde_json::json!({
            "file_path": "subdir/new.txt",
            "content": "created",
        }))
        .await;

    assert!(result.success);
    assert!(workspace.path().join("subdir").join("new.txt").exists());
}
