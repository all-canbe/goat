//! Web search tool — DuckDuckGo Instant Answer API
//!
//! Provides web search capability via DuckDuckGo's free API.
//! Includes rate limiting (1 req/sec) and result truncation.

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use super::registry::{Tool, ToolResult};
use crate::security::approval::ToolCategory;

/// DuckDuckGo Instant Answer API response
#[derive(Debug, Deserialize)]
struct DuckDuckGoResponse {
    #[serde(rename = "AbstractText", default)]
    abstract_text: String,
    #[serde(rename = "RelatedTopics", default)]
    related_topics: Vec<RelatedTopic>,
}

/// A single related topic from DuckDuckGo
#[derive(Debug, Deserialize)]
struct RelatedTopic {
    #[serde(rename = "Text", default)]
    text: String,
}

/// WebSearch tool using DuckDuckGo's free Instant Answer API.
///
/// Features:
/// - Rate limiting: min 1 second between calls
/// - Result truncation: max 4096 characters
/// - Returns abstract + up to 10 related topics
pub struct WebSearchTool {
    client: reqwest::Client,
    last_call: Mutex<Instant>,
}

impl WebSearchTool {
    /// Create a new WebSearchTool with the given HTTP client.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            last_call: Mutex::new(Instant::now() - Duration::from_secs(2)),
        }
    }
}

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web using DuckDuckGo. Returns abstract and related topics. "
    }

    fn parameters(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                }
            },
            "required": ["query"]
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Network
    }

    async fn execute(&self, args: serde_json::Value) -> ToolResult {
        // Rate limiting: enforce at least 1 second between calls
        let wait_duration = {
            let last = self.last_call.lock().unwrap();
            let elapsed = last.elapsed();
            if elapsed < Duration::from_secs(1) {
                Some(Duration::from_secs(1) - elapsed)
            } else {
                None
            }
        };
        // Update last_call time immediately (before any await)
        {
            let mut last = self.last_call.lock().unwrap();
            *last = Instant::now();
        }
        // Sleep outside the lock to maintain Send
        if let Some(dur) = wait_duration {
            tokio::time::sleep(dur).await;
        }

        let query = args["query"].as_str().unwrap_or("");
        if query.is_empty() {
            return ToolResult::error("query is required", "query is required");
        }

        let url = format!(
            "https://api.duckduckgo.com/?q={}&format=json&no_html=1",
            urlencoding(query)
        );

        match self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
        {
            Ok(resp) => {
                if !resp.status().is_success() {
                    return ToolResult::error(
                        format!("DuckDuckGo API returned HTTP {}", resp.status()),
                        "DuckDuckGo API error",
                    );
                }
                match resp.json::<DuckDuckGoResponse>().await {
                    Ok(dd) => {
                        let mut result = String::new();

                        // Append abstract
                        if !dd.abstract_text.is_empty() {
                            result.push_str(&dd.abstract_text);
                            result.push_str("\n\n");
                        }

                        // Append related topics (max 10)
                        for (i, topic) in dd.related_topics.iter().take(10).enumerate() {
                            if !topic.text.is_empty() {
                                result.push_str(&format!("{}. {}\n", i + 1, topic.text));
                            }
                        }

                        // Truncate to 4096 characters, safe on UTF-8 boundaries
                        if result.chars().count() > 4096 {
                            result = result.chars().take(4096).collect();
                            result.push_str("...");
                        }

                        if result.trim().is_empty() {
                            ToolResult::success("No results found for query.")
                        } else {
                            ToolResult::success(result)
                        }
                    }
                    Err(e) => ToolResult::error(
                        format!("Failed to parse DuckDuckGo response: {}", e),
                        format!("Parse error: {}", e),
                    ),
                }
            }
            Err(e) => ToolResult::error(
                format!("DuckDuckGo request failed: {}", e),
                format!("Request error: {}", e),
            ),
        }
    }
}

/// Simple URL encoding for the query parameter.
/// DuckDuckGo API needs spaces as %20 and special chars encoded.
fn urlencoding(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            ' ' => "%20".to_string(),
            _ => format!("%{:02X}", c as u8),
        })
        .collect()
}
