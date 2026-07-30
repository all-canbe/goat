//! Web fetch tool — HTTP GET with SSRF protection, Content-Type routing, streaming body & LRU cache
//!
//! Security features:
//! - SSRF: rejects loopback, private, link-local, metadata IPs (DNS resolution + IP check)
//! - Manual redirect handling with per-hop SSRF re-validation and hop limit
//! - Content-Type based routing: HTML→text, text/* & JSON→plain, others→error
//! - Streaming body read with 1 MiB hard limit (never fully downloads oversized responses)
//! - CancellationToken support for cooperative cancellation during fetch, stream & cache write

use async_trait::async_trait;
use serde_json::json;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::registry::{Tool, ToolResult};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Maximum response body size (1 MiB)
const MAX_CONTENT_SIZE: usize = 1_048_576;
/// Maximum number of redirect hops
const MAX_REDIRECTS: u32 = 5;
/// Cache TTL (15 minutes)
const CACHE_TTL: Duration = Duration::from_secs(900);
/// Maximum cache entries (true LRU eviction)
const MAX_CACHE_ENTRIES: usize = 128;
/// HTTP request timeout
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

// ---------------------------------------------------------------------------
// LRU Cache
// ---------------------------------------------------------------------------

/// A cached web page entry
struct CachedPage {
    content: String,
    cached_at: Instant,
}

/// Simple LRU cache that evicts the least recently used entry when full.
struct LruCache {
    map: HashMap<String, (CachedPage, usize)>,  // (entry, order_token)
    order: VecDeque<String>,                     // front = most recently used
    capacity: usize,
    counter: usize,                              // monotonically increasing order token
}

impl LruCache {
    fn new(capacity: usize) -> Self {
        Self {
            map: HashMap::with_capacity(capacity),
            order: VecDeque::with_capacity(capacity),
            capacity,
            counter: 0,
        }
    }

    /// Get entry by key. Returns `None` if missing or expired.
    /// Refreshes access order on hit.
    fn get(&mut self, key: &str) -> Option<&CachedPage> {
        if let Some((entry, _)) = self.map.get(key) {
            if entry.cached_at.elapsed() < CACHE_TTL {
                // Refresh access order
                self.touch(key);
                // Need reborrow after touch
                self.map.get(key).map(|(e, _)| e)
            } else {
                // Expired — remove
                self.remove(key);
                None
            }
        } else {
            None
        }
    }

    /// Insert or update an entry.
    fn insert(&mut self, key: String, entry: CachedPage) {
        if self.map.contains_key(&key) {
            // Update existing — move to front
            self.map.insert(key.clone(), (entry, self.counter));
            self.counter += 1;
            self.touch(&key);
        } else {
            // Evict if at capacity
            if self.map.len() >= self.capacity {
                if let Some(lru_key) = self.order.pop_back() {
                    self.map.remove(&lru_key);
                }
            }
            self.map.insert(key.clone(), (entry, self.counter));
            self.counter += 1;
            self.order.push_front(key);
        }
    }

    /// Remove a key from the cache.
    fn remove(&mut self, key: &str) {
        self.map.remove(key);
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
        }
    }

    /// Move a key to the front of the order list.
    fn touch(&mut self, key: &str) {
        if let Some(pos) = self.order.iter().position(|k| k == key) {
            self.order.remove(pos);
            self.order.push_front(key.to_string());
        }
        if let Some((_, token)) = self.map.get_mut(key) {
            *token = self.counter;
            self.counter += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// WebFetchTool
// ---------------------------------------------------------------------------

/// WebFetch tool with SSRF protection, Content-Type routing, streaming & cancellation.
///
/// Features:
/// - SSRF: rejects loopback, private, link-local, metadata IP ranges
/// - Manual redirect: checks each hop for SSRF, max 5 redirects
/// - Content-Type routing: HTML → markdown/plain text, text/*/JSON → raw, others → error
/// - Streaming body: reads in chunks, stops at 1 MiB
/// - CancellationToken: cooperative cancellation during fetch, stream & cache write
/// - True LRU cache with 15-minute TTL
pub struct WebFetchTool {
    client: reqwest::Client,
    cache: Mutex<LruCache>,
    cancellation: Option<CancellationToken>,
    ssrf_checker: Arc<dyn SsrfChecker>,
}

/// Build the dedicated reqwest client for `WebFetchTool`.
///
/// Auto-redirect is disabled (`Policy::none`) so `fetch_url` can validate
/// every redirect hop against the SSRF policy. Other tools (WebSearch, LLM
/// providers) keep their own client strategy untouched.
pub fn web_fetch_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .expect("static WebFetch client configuration must be valid")
}

// ---------------------------------------------------------------------------
// SSRF checker abstraction (injectable for testing)
// ---------------------------------------------------------------------------

/// SSRF validation policy — decides whether a host may be fetched.
///
/// `DefaultSsrfChecker` is used in production. Tests inject a mock so they
/// can simulate "public entry allowed, loopback redirect blocked" without a
/// real public IP (every local TCP fixture binds 127.0.0.1).
#[async_trait]
pub trait SsrfChecker: Send + Sync {
    /// Validate `host`. `Ok(())` allows the fetch; `Err(msg)` blocks it.
    async fn check(&self, host: &str) -> Result<(), String>;
}

/// Production SSRF checker — delegates to [`WebFetchTool::check_ssrf`].
pub struct DefaultSsrfChecker;

#[async_trait]
impl SsrfChecker for DefaultSsrfChecker {
    async fn check(&self, host: &str) -> Result<(), String> {
        WebFetchTool::check_ssrf(host).await
    }
}

/// Map a `fetch_url`/`stream_body` error message to a stable error code.
fn classify_fetch_error(err: &str) -> &'static str {
    if err.starts_with("ssrf_blocked") {
        "ssrf_blocked"
    } else if err.starts_with("too_many_redirects") {
        "too_many_redirects"
    } else if err.starts_with("response_too_large") {
        "response_too_large"
    } else {
        "fetch_error"
    }
}

impl WebFetchTool {
    /// Create a new WebFetchTool with an optional cancellation token.
    ///
    /// `client` should come from [`web_fetch_client`] so auto-redirect is
    /// disabled — `fetch_url` validates each redirect hop manually.
    pub fn new(client: reqwest::Client, cancellation: Option<CancellationToken>) -> Self {
        Self::new_with_ssrf_checker(client, cancellation, Arc::new(DefaultSsrfChecker))
    }

    /// Create a WebFetchTool with a custom [`SsrfChecker`].
    ///
    /// Production code uses [`DefaultSsrfChecker`] via [`new`]; tests inject
    /// a mock to simulate "public entry allowed, loopback redirect blocked"
    /// without a real public IP.
    pub fn new_with_ssrf_checker(
        client: reqwest::Client,
        cancellation: Option<CancellationToken>,
        checker: Arc<dyn SsrfChecker>,
    ) -> Self {
        Self {
            client,
            cache: Mutex::new(LruCache::new(MAX_CACHE_ENTRIES)),
            cancellation,
            ssrf_checker: checker,
        }
    }

    /// Cache key derived from the entry (request) URL.
    ///
    /// Uses the full URL string (not a truncated hash) so URLs sharing a long
    /// prefix don't collide. The same key is used for both lookup and insert
    /// so repeated requests hit the cache even when the fetch was redirected.
    fn cache_key(url: &str) -> String {
        url.to_string()
    }

    /// Validate URL scheme — only `http` / `https` allowed.
    fn validate_url(url: &str) -> Result<url::Url, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL: {}", e))?;
        match parsed.scheme() {
            "http" | "https" => Ok(parsed),
            other => Err(format!(
                "Scheme '{}' is not allowed. Only http and https are supported.",
                other
            )),
        }
    }

    /// SSRF check: resolve host to IP addresses and reject blocked ranges.
    async fn check_ssrf(host: &str) -> Result<(), String> {
        // If the host is already a literal IP, check directly (no DNS).
        if let Ok(ip) = host.parse::<IpAddr>() {
            if is_ip_blocked(&ip) {
                return Err(format!(
                    "ssrf_blocked: address '{}' is blocked",
                    host
                ));
            }
            return Ok(());
        }

        // DNS resolution — check every returned address.
        // Use port 80 for lookup (port is ignored for address resolution).
        let addrs = tokio::net::lookup_host((host, 80))
            .await
            .map_err(|e| format!("DNS resolution failed for '{}': {}", host, e))?;

        let mut resolved = Vec::new();
        for addr in addrs {
            if is_ip_blocked(&addr.ip()) {
                return Err(format!(
                    "ssrf_blocked: '{}' resolves to blocked address ({})",
                    host,
                    addr.ip()
                ));
            }
            resolved.push(addr.ip());
        }

        if resolved.is_empty() {
            return Err(format!("DNS resolution for '{}' returned no addresses", host));
        }

        Ok(())
    }

    /// Fetch a URL with SSRF protection and manual redirect handling.
    ///
    /// Returns the final `Response` and the final URL string (after redirects).
    async fn fetch_url(&self, url: &str) -> Result<(reqwest::Response, String), String> {
        // Check cancellation before starting
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return Err("Operation cancelled".to_string());
            }
        }

        let mut current_url = url.to_string();
        let mut redirect_count: u32 = 0;

        loop {
            // 1. Validate URL scheme
            let parsed = Self::validate_url(&current_url)?;

            // 2. SSRF check on the target host (per-hop, injectable)
            if let Some(host) = parsed.host_str() {
                self.ssrf_checker.check(host).await?;
            }

            // 3. Check cancellation before sending request
            if let Some(token) = &self.cancellation {
                if token.is_cancelled() {
                    return Err("Operation cancelled".to_string());
                }
            }

            // 4. Send request
            let resp = self
                .client
                .get(&current_url)
                .header("User-Agent", "rgoat/0.1")
                .timeout(REQUEST_TIMEOUT)
                .send()
                .await
                .map_err(|e| format!("Request failed: {}", e))?;

            // 5. Handle redirects manually
            if resp.status().is_redirection() {
                redirect_count += 1;
                if redirect_count > MAX_REDIRECTS {
                    return Err(format!("too_many_redirects: limit {}", MAX_REDIRECTS));
                }

                let location = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| "Redirect response without Location header".to_string())?;

                // Resolve relative redirect against the current URL
                current_url = parsed
                    .join(location)
                    .map_err(|e| format!("Invalid redirect URL '{}': {}", location, e))?
                    .to_string();

                continue; // loop: SSRF-check the redirect target
            }

            return Ok((resp, current_url));
        }
    }

    /// Stream the response body in chunks, stopping at `MAX_CONTENT_SIZE`.
    ///
    /// Checks `CancellationToken` between chunks for cooperative cancellation.
    async fn stream_body(&self, mut resp: reqwest::Response) -> Result<String, String> {
        // Content-Length preflight: reject before reading any body bytes so
        // oversized responses are never downloaded.
        if let Some(cl) = resp.headers().get(reqwest::header::CONTENT_LENGTH) {
            if let Ok(s) = cl.to_str() {
                if let Ok(n) = s.trim().parse::<usize>() {
                    if n > MAX_CONTENT_SIZE {
                        return Err("response_too_large".to_string());
                    }
                }
            }
        }

        let mut body = String::new();
        let mut total: usize = 0;

        loop {
            let chunk = match resp.chunk().await {
                Ok(Some(c)) => c,
                Ok(None) => break, // end of stream
                Err(e) => return Err(format!("Stream read error: {}", e)),
            };

            total += chunk.len();
            if total > MAX_CONTENT_SIZE {
                // Hard limit exceeded mid-stream: do not cache partial content.
                return Err("response_too_large".to_string());
            }

            body.push_str(&String::from_utf8_lossy(&chunk));

            // Check cancellation between chunks
            if let Some(token) = &self.cancellation {
                if token.is_cancelled() {
                    return Err("Operation cancelled".to_string());
                }
            }
        }

        Ok(body)
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetches content from a URL and converts HTML to plain text. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch (http/https only)"
                }
            },
            "required": ["url"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }

    fn execution_mode(&self) -> super::registry::ExecutionMode {
        super::registry::ExecutionMode::Parallel
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let url = args["url"].as_str().unwrap_or("");
        if url.is_empty() {
            return ToolResult::error("url is required", "url is required");
        }

        // Check cancellation before any operation
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation cancelled", "cancelled");
            }
        }

        let lookup_key = Self::cache_key(url);

        // Check LRU cache (refreshes access order on hit)
        {
            let mut cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(&lookup_key) {
                return ToolResult::success(entry.content.clone());
            }
        }

        // Fetch with SSRF protection, redirect handling, and cancellation
        let (resp, _final_url) = match self.fetch_url(url).await {
            Ok(v) => v,
            Err(e) => {
                return ToolResult::error(
                    format!("Failed to fetch URL: {}", e),
                    classify_fetch_error(&e),
                );
            }
        };

        if !resp.status().is_success() {
            return ToolResult::error(
                format!("HTTP error: {}", resp.status()),
                format!("HTTP {}", resp.status()),
            );
        }

        // ── Content-Type based routing ──
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html");

        let is_html = content_type.contains("text/html");
        let is_text = content_type.starts_with("text/");
        let is_json =
            content_type.contains("application/json") || content_type.contains("application/problem+json");

        // Check cancellation before streaming body
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation cancelled", "cancelled");
            }
        }

        let text = if is_html || is_text || is_json {
            // Stream body with size limit & cancellation
            let body = match self.stream_body(resp).await {
                Ok(b) => b,
                Err(e) => {
                    let code = if e.contains("response_too_large") {
                        "response_too_large"
                    } else {
                        "read_error"
                    };
                    return ToolResult::error(
                        format!("Failed to read response body: {}", e),
                        code,
                    );
                }
            };

            if is_html {
                html_to_text(&body)
            } else if is_json {
                // Pretty-print JSON for readability
                match serde_json::from_str::<serde_json::Value>(&body) {
                    Ok(v) => serde_json::to_string_pretty(&v).unwrap_or(body),
                    Err(_) => body,
                }
            } else {
                body
            }
        } else {
            return ToolResult::error(
                format!(
                    "Unsupported Content-Type: '{}'. Only HTML, text, and JSON are supported.",
                    content_type
                ),
                "unsupported_content_type",
            );
        };

        // Check cancellation before cache write
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation cancelled", "cancelled");
            }
        }

        // Store in LRU cache under the same key used for lookups (the
        // entry/request URL) so repeated requests hit the cache even when
        // the fetch was redirected.
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(
                lookup_key.clone(),
                CachedPage {
                    content: text.clone(),
                    cached_at: Instant::now(),
                },
            );
        }

        ToolResult::success(text)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// SSRF IP range detection
// ═══════════════════════════════════════════════════════════════════════════

/// Check whether an IP address is in a blocked range (SSRF protection).
///
/// Blocked ranges:
/// - IPv4: loopback (127.0.0.0/8), private (10/8, 172.16/12, 192.168/16),
///         link-local / metadata (169.254.0.0/16)
/// - IPv6: loopback (::1), link-local (fe80::/10), unique-local (fc00::/7),
///         unspecified (::)
fn is_ip_blocked(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            // Loopback: 127.0.0.0/8
            octets[0] == 127
            // Private: 10.0.0.0/8
            || octets[0] == 10
            // Private: 172.16.0.0/12
            || (octets[0] == 172 && (octets[1] & 0xF0) == 16)
            // Private: 192.168.0.0/16
            || (octets[0] == 192 && octets[1] == 168)
            // Link-local (incl. metadata 169.254.169.254): 169.254.0.0/16
            || (octets[0] == 169 && octets[1] == 254)
        }
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            // Loopback: ::1
            segs == [0, 0, 0, 0, 0, 0, 0, 1]
            // Unspecified: ::
            || segs == [0, 0, 0, 0, 0, 0, 0, 0]
            // Link-local: fe80::/10
            || (segs[0] & 0xFFC0) == 0xFE80
            // Unique-local: fc00::/7
            || (segs[0] & 0xFE00) == 0xFC00
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// HTML → Markdown / Plain Text Content Extraction
// ═══════════════════════════════════════════════════════════════════════════

/// States for the HTML-to-markdown parser
enum ParseState {
    Text,
    Tag,
    Script,
    Style,
    /// Inside `<pre>` — passthrough raw text
    Pre,
    /// Inside a boilerplate region — suppress output
    Skip,
}

/// Tags that indicate boilerplate/suppressed regions.
fn is_skip_tag(tag_name: &str) -> bool {
    let t = tag_name.trim_start_matches('/');
    matches!(
        t,
        "nav" | "header" | "footer" | "aside" | "noscript"
    )
}

/// Check if a tag is an opening heading tag (h1-h6).
fn heading_level(tag_lower: &str) -> Option<usize> {
    let t = tag_lower.trim_start_matches('/');
    match t {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Check if a tag name is a closing tag (starts with '/').
fn is_closing_tag(tag_lower: &str) -> bool {
    tag_lower.starts_with('/')
}

/// Convert HTML to markdown-quality plain text.
///
/// Features:
/// - Strips `<script>`, `<style>`, and boilerplate regions (nav, header, footer, aside)
/// - Converts headings to `# ` markdown format
/// - Wraps `<pre><code>` in ``` fences
/// - Formats `<a>` links as `[text](url)`
/// - Handles `<img>` alt text
/// - Block-level tags produce paragraph breaks
/// - Whitespace is normalized (single spaces, blank lines between blocks)
fn html_to_text(html: &str) -> String {
    let mut state = ParseState::Text;
    let mut output = String::with_capacity(html.len() / 3);
    let mut tag_buf = String::new();
    let mut space_run = false;
    let mut blank_line = false;

    // Link tracking
    let mut link_text = String::new();
    let mut link_url = String::new();
    let mut in_a = false;

    // Skip region depth (suppress output when > 0)
    let mut skip_depth: usize = 0;

    // Heading tracking
    let mut current_heading: Option<usize> = None;
    let mut heading_text = String::new();

    // Code block tracking
    let mut in_pre = false;
    let mut code_content = String::new();

    // List tracking
    let mut in_li = false;

    for ch in html.chars() {
        match state {
            ParseState::Text => {
                if ch == '<' {
                    state = ParseState::Tag;
                    tag_buf.clear();
                } else if skip_depth > 0 {
                    // Suppress output inside boilerplate
                } else if let Some(_hl) = current_heading {
                    // Collect heading text
                    if ch == '\n' {
                        heading_text.push(' ');
                    } else {
                        heading_text.push(ch);
                    }
                } else if in_a {
                    link_text.push(ch);
                    if ch.is_whitespace() {
                        if !space_run {
                            output.push(' ');
                            space_run = true;
                        }
                    } else {
                        output.push(ch);
                        space_run = false;
                    }
                } else if ch.is_whitespace() {
                    if !space_run {
                        output.push(' ');
                        space_run = true;
                        blank_line = false;
                    }
                } else {
                    output.push(ch);
                    space_run = false;
                    blank_line = false;
                }
            }
            ParseState::Tag => {
                if ch == '>' {
                    let tag_lower = tag_buf.to_lowercase();
                    handle_tag(
                        &tag_lower,
                        &tag_buf,
                        &mut state,
                        &mut output,
                        &mut space_run,
                        &mut blank_line,
                        &mut in_a,
                        &mut link_text,
                        &mut link_url,
                        &mut skip_depth,
                        &mut current_heading,
                        &mut heading_text,
                        &mut in_pre,
                        &mut code_content,
                        &mut in_li,
                    );
                } else {
                    tag_buf.push(ch);
                }
            }
            ParseState::Script => {
                if ch == '>' {
                    let tag_lower = tag_buf.to_lowercase();
                    if tag_lower == "/script" || tag_lower.starts_with("/script") {
                        state = ParseState::Text;
                    }
                    tag_buf.clear();
                } else if ch == '<' {
                    tag_buf.clear();
                } else {
                    tag_buf.push(ch);
                }
            }
            ParseState::Style => {
                if ch == '>' {
                    let tag_lower = tag_buf.to_lowercase();
                    if tag_lower == "/style" || tag_lower.starts_with("/style") {
                        state = ParseState::Text;
                    }
                    tag_buf.clear();
                } else if ch == '<' {
                    tag_buf.clear();
                } else {
                    tag_buf.push(ch);
                }
            }
            ParseState::Pre => {
                if ch == '<' {
                    state = ParseState::Tag;
                    tag_buf.clear();
                } else {
                    code_content.push(ch);
                }
            }
            ParseState::Skip => {
                // Inside boilerplate: wait for closing tag
                if ch == '>' {
                    let tag_lower = tag_buf.to_lowercase();
                    if is_closing_tag(&tag_lower) {
                        let tag_name = tag_lower.trim_start_matches('/');
                        if is_skip_tag(tag_name) {
                            skip_depth = skip_depth.saturating_sub(1);
                        }
                        if skip_depth == 0 {
                            state = ParseState::Text;
                        }
                    }
                    tag_buf.clear();
                } else if ch == '<' {
                    tag_buf.clear();
                } else {
                    tag_buf.push(ch);
                }
            }
        }
    }

    // Final post-processing
    finalize_output(output)
}

/// Handle a parsed HTML tag (called when '>' is encountered in Tag state).
#[allow(clippy::too_many_arguments)]
fn handle_tag(
    tag_lower: &str,
    tag_buf: &str,
    state: &mut ParseState,
    output: &mut String,
    space_run: &mut bool,
    blank_line: &mut bool,
    in_a: &mut bool,
    link_text: &mut String,
    link_url: &mut String,
    skip_depth: &mut usize,
    current_heading: &mut Option<usize>,
    heading_text: &mut String,
    in_pre: &mut bool,
    code_content: &mut String,
    in_li: &mut bool,
) {
    let is_closing = is_closing_tag(tag_lower);
    let tag_name = if is_closing {
        tag_lower.trim_start_matches('/')
    } else {
        tag_lower
    };

    // ── Script / Style ──
    if tag_name == "script" && !is_closing {
        *state = ParseState::Script;
        return;
    }
    if tag_name == "style" && !is_closing {
        *state = ParseState::Style;
        return;
    }

    // ── Skip boilerplate regions ──
    if is_skip_tag(tag_name) {
        if is_closing {
            *skip_depth = skip_depth.saturating_sub(1);
        } else {
            *skip_depth += 1;
        }
        if *skip_depth > 0 {
            *state = ParseState::Skip;
        }
        return;
    }

    // If inside a skip region, ignore all other tags
    if *skip_depth > 0 {
        return;
    }

    // ── Headings ──
    if let Some(hl) = heading_level(tag_name) {
        if is_closing {
            // Emit markdown heading
            let prefix = "#".repeat(hl);
            let text = heading_text.trim();
            if !text.is_empty() {
                if !output.is_empty() {
                    emit_newline(output, blank_line, false);
                }
                output.push_str(&format!("{} {}\n", prefix, text));
                *blank_line = true;
                *space_run = true;
            }
            *current_heading = None;
            heading_text.clear();
        } else {
            *current_heading = Some(hl);
            heading_text.clear();
        }
        *state = ParseState::Text;
        return;
    }

    // ── Code blocks ──
    if tag_name == "pre" && !is_closing {
        *in_pre = true;
        code_content.clear();
        *state = ParseState::Pre;
        return;
    }
    if tag_name == "pre" && is_closing {
        *in_pre = false;
        // Emit code block
        let code = code_content.trim();
        if !code.is_empty() {
            if !output.is_empty() {
                emit_newline(output, blank_line, false);
            }
            output.push_str("```\n");
            output.push_str(code);
            if !code.ends_with('\n') {
                output.push('\n');
            }
            output.push_str("```\n");
            *blank_line = true;
            *space_run = true;
        }
        code_content.clear();
        *state = ParseState::Text;
        return;
    }

    // ── Links: <a href="..."> ──
    if tag_name == "a" && !is_closing {
        *in_a = true;
        link_text.clear();
        link_url.clear();
        // Extract href attribute
        if let Some(href_start) = tag_buf.to_lowercase().find("href=") {
            let rest = &tag_buf[href_start + 5..];
            let delim = rest.chars().next().unwrap_or('"');
            if let Some(end) = rest[1..].find(delim) {
                *link_url = rest[1..=end].to_string();
            }
        }
        *state = ParseState::Text;
        return;
    }
    if tag_name == "a" && is_closing {
        *in_a = false;
        if !link_text.is_empty() && !link_url.is_empty() {
            output.push_str(&format!(" [{}]({})", link_text.trim(), *link_url));
        }
        link_text.clear();
        link_url.clear();
        *state = ParseState::Text;
        return;
    }

    // ── Images: <img alt="..."> ──
    if tag_name.starts_with("img ") || tag_name == "img" {
        if let Some(alt_start) = tag_buf.to_lowercase().find("alt=") {
            let rest = &tag_buf[alt_start + 4..];
            let delim = rest.chars().next().unwrap_or('"');
            if let Some(end) = rest[1..].find(delim) {
                output.push_str(&format!("[Image: {}]", &rest[1..=end]));
                *space_run = true;
            }
        }
        *state = ParseState::Text;
        return;
    }

    // ── List items ──
    if tag_name == "li" && !is_closing {
        *in_li = true;
        if !output.is_empty() {
            emit_newline(output, blank_line, false);
        }
        output.push_str("- ");
        *space_run = false;
        *state = ParseState::Text;
        return;
    }
    if tag_name == "li" && is_closing {
        *in_li = false;
        output.push('\n');
        *blank_line = false;
        *state = ParseState::Text;
        return;
    }

    // ── Block-level tags → paragraph breaks ──
    if is_block_tag(tag_name, is_closing) {
        emit_newline(output, blank_line, true);
        *state = ParseState::Text;
        return;
    }

    // Default: back to Text
    *state = ParseState::Text;
}

/// Emit a newline, avoiding excessive blank lines.
fn emit_newline(output: &mut String, blank_line: &mut bool, is_paragraph: bool) {
    if is_paragraph {
        // Paragraphs get a blank line
        if !*blank_line {
            output.push('\n');
        }
        *blank_line = true;
    } else if !output.ends_with('\n') {
        output.push('\n');
    }
}

/// Check if a tag is a block-level element that should trigger a newline.
fn is_block_tag(tag_lower: &str, is_closing: bool) -> bool {
    let t = if is_closing {
        tag_lower.trim_start_matches('/')
    } else {
        tag_lower
    };
    matches!(
        t,
        "p" | "div" | "section" | "article" | "blockquote" | "hr"
            | "br" | "tr" | "/tr" | "table" | "/table"
            | "ul" | "/ul" | "ol" | "/ol" | "dl" | "/dl"
    ) || (t.starts_with("h") && t.len() == 2 && t.as_bytes().get(1).map_or(false, |b| b.is_ascii_digit()))
}

/// Final cleanup: normalize whitespace, remove empty lines.
fn finalize_output(raw: String) -> String {
    raw.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

// ═══════════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn cached(content: &str) -> CachedPage {
        CachedPage {
            content: content.into(),
            cached_at: Instant::now(),
        }
    }

    /// Read and discard the client request line + headers (up to the blank
    /// `\r\n\r\n` terminator) so the server doesn't race ahead of reqwest's
    /// request write and reset the connection mid-handshake.
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

    #[test]
    fn lru_cache_evicts_least_recently_used() {
        let mut cache = LruCache::new(2);
        cache.insert("http://a/".into(), cached("A"));
        cache.insert("http://b/".into(), cached("B"));
        // Touch "a" so "b" becomes the least recently used entry.
        let _ = cache.get("http://a/");
        cache.insert("http://c/".into(), cached("C"));

        assert!(
            cache.get("http://b/").is_none(),
            "LRU victim 'b' must be evicted"
        );
        assert!(cache.get("http://a/").is_some(), "recently used 'a' must remain");
        assert!(cache.get("http://c/").is_some());
    }

    #[test]
    fn lru_cache_key_is_full_url_string() {
        // The cache must key on the full URL string (not a truncated hash),
        // so URLs sharing a long prefix don't collide.
        let mut cache = LruCache::new(4);
        cache.insert("http://example.com/page1".into(), cached("one"));
        cache.insert("http://example.com/page2".into(), cached("two"));

        assert_eq!(
            cache.get("http://example.com/page1").map(|c| c.content.as_str()),
            Some("one")
        );
        assert_eq!(
            cache.get("http://example.com/page2").map(|c| c.content.as_str()),
            Some("two")
        );
    }

    /// Spawn a server returning `Content-Length: 1048577` (1 byte over the
    /// 1 MiB limit) and no body. The tool must reject before streaming.
    async fn spawn_oversize_content_length() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                drain_request(&mut sock).await;
                let resp = "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 1048577\r\n\r\n";
                let _ = sock.write_all(resp.as_bytes()).await;
                // No body — preflight must reject before any body read.
                // Hold the socket open until reqwest finishes parsing headers.
                let mut buf = [0u8; 64];
                let _ = sock.read(&mut buf).await;
            }
        });
        format!("http://127.0.0.1:{}/", port)
    }

    #[tokio::test]
    async fn stream_body_rejects_oversize_content_length() {
        let url = spawn_oversize_content_length().await;
        let tool = WebFetchTool::new(web_fetch_client(), None);
        // Fetch directly with the no-redirect client, bypassing fetch_url's
        // SSRF gate (which blocks loopback) to isolate body-size preflight.
        let resp = web_fetch_client().get(&url).send().await.unwrap();
        let result = tool.stream_body(resp).await;

        assert!(result.is_err(), "oversize Content-Length must be rejected");
        assert!(
            result.unwrap_err().contains("response_too_large"),
            "error code must be response_too_large"
        );
    }

    /// Spawn a server emitting >1 MiB with no Content-Length (body delimited by
    /// connection close). This forces the streaming accumulator to hit the
    /// hard limit mid-stream without a preflight Content-Length check — the
    /// same code path exercised by a chunked transfer of unknown size.
    async fn spawn_oversize_streaming_body() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            if let Ok((mut sock, _)) = listener.accept().await {
                drain_request(&mut sock).await;
                let header =
                    "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nConnection: close\r\n\r\n";
                let _ = sock.write_all(header.as_bytes()).await;
                let block = vec![b'a'; 65536];
                // 32 * 65536 = 2,097,152 bytes > 1 MiB (1,048,576).
                for _ in 0..32 {
                    if sock.write_all(&block).await.is_err() {
                        break;
                    }
                }
            }
        });
        format!("http://127.0.0.1:{}/", port)
    }

    #[tokio::test]
    async fn stream_body_rejects_oversize_stream() {
        let url = spawn_oversize_streaming_body().await;
        let tool = WebFetchTool::new(web_fetch_client(), None);
        let resp = web_fetch_client().get(&url).send().await.unwrap();
        let result = tool.stream_body(resp).await;

        assert!(result.is_err(), "oversize stream must be rejected");
        assert!(
            result.unwrap_err().contains("response_too_large"),
            "error code must be response_too_large"
        );
    }
}
