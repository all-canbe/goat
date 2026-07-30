//! Web search tool — DuckDuckGo HTML free search
//!
//! Built-in WebSearch is fixed to DuckDuckGo HTML scraping (no API key
//! required). For professional, paid, authenticated, or enterprise search,
//! use an MCP search tool.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::registry::{Tool, ToolResult};
use crate::core::cancellation::CancellationToken;
use crate::security::approval::ToolCategory;

/// A single search result
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

/// Web search provider trait
#[async_trait]
pub trait WebSearchProvider: Send + Sync {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String>;
}

// ============================================================================
// DuckDuckGo HTML provider (no API key required)
// ============================================================================

struct DuckDuckGoHtmlProvider {
    client: reqwest::Client,
}

impl DuckDuckGoHtmlProvider {
    fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl WebSearchProvider for DuckDuckGoHtmlProvider {
    async fn search(&self, query: &str, max_results: usize) -> Result<Vec<SearchResult>, String> {
        let url = "https://html.duckduckgo.com/html/";
        let params = [("q", query)];

        let resp = self
            .client
            .post(url)
            .form(&params)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| format!("DuckDuckGo request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("DuckDuckGo returned HTTP {}", resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read response: {}", e))?;

        Ok(parse_ddg_html(&body, max_results))
    }
}

/// Minimal HTML parser for DuckDuckGo search results page
fn parse_ddg_html(html: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // Find each result block: <div class="result">
    for block in html.split(r#"<div class="result" "#).skip(1) {
        if results.len() >= max_results {
            break;
        }

        // Extract title from <a class="result__a">
        let title = extract_between(block, r#"<a class="result__a" "#, "</a>")
            .map(|s| strip_html_tags(&s))
            .unwrap_or_default();

        // Extract URL from href= inside the <a> tag
        let url = extract_between(block, r#"href=""#, r#""#).unwrap_or_default();

        // Extract snippet from <a class="result__snippet">
        let snippet = extract_between(block, r#"<a class="result__snippet" "#, "</a>")
            .map(|s| strip_html_tags(&s))
            .unwrap_or_default();

        if !title.is_empty() {
            results.push(SearchResult {
                title: html_unescape(&title),
                url: html_unescape(&url),
                snippet: html_unescape(&snippet),
            });
        }
    }

    results
}

/// Extract text between two markers after start marker
fn extract_between<'a>(text: &'a str, start_marker: &str, end_marker: &str) -> Option<String> {
    let start = text.find(start_marker)?;
    let after_start = &text[start + start_marker.len()..];
    // Find the closing > of the opening tag first
    let tag_end = after_start.find('>')?;
    let after_tag = &after_start[tag_end + 1..];
    let end = after_tag.find(end_marker)?;
    Some(after_tag[..end].to_string())
}

/// Strip HTML tags from string
fn strip_html_tags(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for ch in s.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

/// Simple HTML unescape
fn html_unescape(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

// ============================================================================
// WebSearchTool
// ============================================================================

pub struct WebSearchTool {
    provider: Box<dyn WebSearchProvider>,
    last_call: Mutex<Instant>,
    cancellation: Option<CancellationToken>,
}

impl WebSearchTool {
    /// Create a new WebSearchTool with the built-in DuckDuckGo HTML provider.
    pub fn new(client: reqwest::Client, cancellation: Option<CancellationToken>) -> Self {
        Self {
            provider: Box::new(DuckDuckGoHtmlProvider::new(client)),
            last_call: Mutex::new(Instant::now() - Duration::from_secs(2)),
            cancellation,
        }
    }

    /// Create with explicit provider (for testing)
    pub fn with_provider(
        provider: Box<dyn WebSearchProvider>,
        cancellation: Option<CancellationToken>,
    ) -> Self {
        Self {
            provider,
            last_call: Mutex::new(Instant::now() - Duration::from_secs(2)),
            cancellation,
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Free DuckDuckGo HTML search. Best-effort results; for higher-quality or authenticated search, use an MCP search tool."
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results (default: 5, max: 20)",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }

    fn execution_mode(&self) -> super::registry::ExecutionMode {
        super::registry::ExecutionMode::Parallel
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        // Check cancellation before starting
        if let Some(token) = &self.cancellation {
            if token.is_cancelled() {
                return ToolResult::error("Operation aborted", "cancelled");
            }
        }

        // Rate limiting: 1 req/sec
        let wait_duration = {
            let last = self.last_call.lock().unwrap();
            let elapsed = last.elapsed();
            if elapsed < Duration::from_secs(1) {
                Some(Duration::from_secs(1) - elapsed)
            } else {
                None
            }
        };
        {
            let mut last = self.last_call.lock().unwrap();
            *last = Instant::now();
        }
        if let Some(dur) = wait_duration {
            tokio::time::sleep(dur).await;
        }

        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return ToolResult::error("query is required", "query is required");
        }

        let max_results = args["max_results"]
            .as_u64()
            .unwrap_or(5)
            .min(20) as usize;

        match self.provider.search(query, max_results).await {
            Ok(results) => {
                let results: Vec<_> = results.into_iter().take(max_results).collect();
                if results.is_empty() {
                    return ToolResult::success("No results found.");
                }

                let mut output = String::new();
                for (i, r) in results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. [{}]({})\n   {}\n\n",
                        i + 1,
                        r.title,
                        r.url,
                        r.snippet
                    ));
                }

                ToolResult::success(output.trim().to_string()).with_metadata(json!({
                    "result_count": results.len()
                }))
            }
            Err(e) => ToolResult::error(
                format!("Web search failed: {}", e),
                format!("search_error: {}", e),
            ),
        }
    }
}
