//! Web fetch tool — HTTP GET + HTML→Text + LRU cache
//!
//! Fetches content from URLs, converts HTML to plain text,
//! and caches results for 15 minutes (900 seconds).
//! Max content size: 1 MB. Max cache entries: 128.

use async_trait::async_trait;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, Instant};

use super::registry::{Tool, ToolResult};
use crate::security::approval::ToolCategory;

/// A cached web page entry
struct CachedPage {
    content: String,
    cached_at: Instant,
}

/// WebFetch tool that downloads and converts web pages to text.
///
/// Features:
/// - LRU-style cache with 15-minute TTL
/// - HTML to plain text conversion via a state machine
/// - Preserves links as markdown `[text](url)`
/// - Strips script and style tags
/// - 1 MB size limit
pub struct WebFetchTool {
    client: reqwest::Client,
    cache: RwLock<HashMap<String, CachedPage>>,
}

impl WebFetchTool {
    /// Create a new WebFetchTool with the given HTTP client.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Generate a short cache key from the URL using SHA-256 (first 16 hex chars).
    fn cache_key(url: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(url.as_bytes());
        let full = format!("{:x}", hasher.finalize());
        full[..16.min(full.len())].to_string()
    }
}

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetches content from a URL and converts HTML to plain text. Results are cached for 15 minutes. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                }
            },
            "required": ["url"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        let url = args["url"].as_str().unwrap_or("");
        if url.is_empty() {
            return ToolResult::error("url is required", "url is required");
        }

        let key = Self::cache_key(url);

        // Check cache (15-minute TTL)
        {
            let cache = self.cache.read().unwrap();
            if let Some(entry) = cache.get(&key) {
                if entry.cached_at.elapsed() < Duration::from_secs(900) {
                    let output = entry.content.clone();
                    return ToolResult::success(output);
                }
            }
        }

        // Fetch the URL
        let resp = match self
            .client
            .get(url)
            .header("User-Agent", "rgoat/0.1")
            .timeout(Duration::from_secs(15))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return ToolResult::error(
                    format!("Failed to fetch URL: {}", e),
                    format!("Request error: {}", e),
                );
            }
        };

        if !resp.status().is_success() {
            return ToolResult::error(
                format!("HTTP error: {}", resp.status()),
                format!("HTTP {}", resp.status()),
            );
        }

        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return ToolResult::error(
                    format!("Failed to read response body: {}", e),
                    format!("Read error: {}", e),
                );
            }
        };

        // Limit to 1 MB
        let bytes = if bytes.len() > 1_048_576 {
            bytes.slice(0..1_048_576)
        } else {
            bytes
        };

        let html = String::from_utf8_lossy(&bytes);
        let text = html_to_text(&html);

        // Store in cache (evict oldest half if full)
        {
            let mut cache = self.cache.write().unwrap();
            if cache.len() >= 128 {
                cache.clear();
            }
            cache.insert(
                key,
                CachedPage {
                    content: text.clone(),
                    cached_at: Instant::now(),
                },
            );
        }

        ToolResult::success(text)
    }
}

// ═══════════════════════════════════════════════════════════
// HTML → Plain Text State Machine
// ═══════════════════════════════════════════════════════════

/// States for the HTML-to-text parser
enum ParseState {
    Text,
    Tag,
    Script,
    Style,
    Pre,
}

/// Convert HTML to plain text using a simple state machine.
///
/// Handles:
/// - `<script>` and `<style>` content stripping
/// - `<pre>` passthrough
/// - `<a href="...">` → `[text](url)` markdown links
/// - `<img alt="...">` → `[Image: alt]`
/// - Block-level tags → newlines
/// - Whitespace normalization
fn html_to_text(html: &str) -> String {
    let mut state = ParseState::Text;
    let mut output = String::with_capacity(html.len() / 4);
    let mut tag_buf = String::new();
    let mut space_run = false;

    // Link tracking
    let mut link_text = String::new();
    let mut link_url = String::new();
    let mut in_a = false;

    for ch in html.chars() {
        match state {
            ParseState::Text => {
                if ch == '<' {
                    state = ParseState::Tag;
                    tag_buf.clear();
                } else if ch.is_whitespace() {
                    if !space_run {
                        output.push(' ');
                        space_run = true;
                    }
                } else {
                    output.push(ch);
                    space_run = false;
                    if in_a {
                        link_text.push(ch);
                    }
                }
            }
            ParseState::Tag => {
                if ch == '>' {
                    let tag_lower = tag_buf.to_lowercase();
                    handle_tag_close(
                        &tag_lower,
                        &tag_buf,
                        &mut state,
                        &mut output,
                        &mut space_run,
                        &mut in_a,
                        &mut link_text,
                        &mut link_url,
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
                output.push(ch);
                if ch == '<' {
                    state = ParseState::Tag;
                    tag_buf.clear();
                }
            }
        }
    }

    // Clean up: remove extra whitespace, empty lines
    output
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Handle the closing of an HTML tag (when '>' is encountered).
fn handle_tag_close(
    tag_lower: &str,
    tag_buf: &str,
    state: &mut ParseState,
    output: &mut String,
    space_run: &mut bool,
    in_a: &mut bool,
    link_text: &mut String,
    link_url: &mut String,
) {
    if tag_lower.starts_with("script") || tag_lower == "script" {
        *state = ParseState::Script;
    } else if tag_lower.starts_with("style") || tag_lower == "style" {
        *state = ParseState::Style;
    } else if tag_lower == "pre" {
        *state = ParseState::Pre;
    } else if tag_lower == "/pre" {
        *state = ParseState::Text;
        output.push('\n');
    } else if tag_lower.starts_with("a ") || tag_lower == "a" {
        // Extract href attribute
        *in_a = true;
        link_text.clear();
        if let Some(href_start) = tag_buf.to_lowercase().find("href=") {
            let rest = &tag_buf[href_start + 5..];
            let delim = rest.chars().next().unwrap_or('"');
            if let Some(end) = rest[1..].find(delim) {
                *link_url = rest[1..=end].to_string();
            }
        }
    } else if tag_lower == "/a" {
        *in_a = false;
        if !link_text.is_empty() && !link_url.is_empty() {
            output.push_str(&format!(" [{}]({})", link_text, link_url));
        }
        link_text.clear();
        link_url.clear();
    } else if tag_lower.starts_with("img ") {
        // Extract alt attribute
        if let Some(alt_start) = tag_buf.to_lowercase().find("alt=") {
            let rest = &tag_buf[alt_start + 4..];
            let delim = rest.chars().next().unwrap_or('"');
            if let Some(end) = rest[1..].find(delim) {
                output.push_str(&format!("[Image: {}]", &rest[1..=end]));
            }
        }
    } else if is_block_tag(tag_lower) {
        output.push('\n');
        *space_run = true;
    }
    *state = ParseState::Text;
}

/// Check if a tag is a block-level element that should trigger a newline.
fn is_block_tag(tag_lower: &str) -> bool {
    tag_lower.starts_with("br")
        || tag_lower == "p"
        || tag_lower == "/p"
        || tag_lower.starts_with("h1")
        || tag_lower.starts_with("h2")
        || tag_lower.starts_with("h3")
        || tag_lower.starts_with("/h1")
        || tag_lower.starts_with("/h2")
        || tag_lower.starts_with("/h3")
        || tag_lower == "li"
        || tag_lower == "/li"
        || tag_lower == "div"
        || tag_lower == "/div"
        || tag_lower == "tr"
        || tag_lower == "/tr"
        || tag_lower == "/table"
}
