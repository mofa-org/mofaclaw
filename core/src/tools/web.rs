//! Web tools: web_search and web_fetch

use super::base::{SimpleTool, ToolInput, ToolResult};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use regex::Regex;
use serde_json::{Value, json};
use std::env;

// For readability extraction
use readability::extractor;

/// User agent string for web requests
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_7_2) AppleWebKit/537.36";

/// Tool to search the web using Brave Search API
pub struct WebSearchTool {
    api_key: Option<String>,
    max_results: usize,
}

impl WebSearchTool {
    /// Create a new WebSearchTool
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            api_key: api_key.or_else(|| env::var("BRAVE_API_KEY").ok()),
            max_results: 5,
        }
    }

    /// Create with custom max results
    pub fn with_max_results(api_key: Option<String>, max_results: usize) -> Self {
        Self {
            api_key: api_key.or_else(|| env::var("BRAVE_API_KEY").ok()),
            max_results: max_results.min(10),
        }
    }
}

#[async_trait]
impl SimpleTool for WebSearchTool {
    fn name(&self) -> &str {
        "web_search"
    }

    fn description(&self) -> &str {
        "Search the web. Returns titles, URLs, and snippets."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query"
                },
                "count": {
                    "type": "integer",
                    "description": "Results (1-10)",
                    "minimum": 1,
                    "maximum": 10
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let api_key = match self.api_key.as_ref() {
            Some(key) => key,
            None => return ToolResult::failure("Error: BRAVE_API_KEY not configured"),
        };

        let query = match input.get_str("query") {
            Some(q) => q,
            None => return ToolResult::failure("Missing 'query' parameter"),
        };

        let count = input
            .get_number("count")
            .unwrap_or(self.max_results as f64)
            .min(10.0)
            .max(1.0) as usize;

        let client = reqwest::Client::new();
        let response = match client
            .get("https://api.search.brave.com/res/v1/web/search")
            .query(&[("q", query), ("count", &count.to_string())])
            .header("Accept", "application/json")
            .header("X-Subscription-Token", api_key)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::failure(format!("Request failed: {}", e)),
        };

        if !response.status().is_success() {
            return ToolResult::failure(format!(
                "Error: API returned status {}",
                response.status()
            ));
        }

        let json: Value = match response.json().await {
            Ok(j) => j,
            Err(e) => return ToolResult::failure(format!("Failed to parse response: {}", e)),
        };

        let results = json["web"]["results"].as_array();

        match results {
            Some(results) if !results.is_empty() => {
                let mut lines = vec![format!("Results for: {}\n", query)];
                for (i, item) in results.iter().take(count).enumerate() {
                    let title = item["title"].as_str().unwrap_or("");
                    let url = item["url"].as_str().unwrap_or("");
                    lines.push(format!("{}. {}", i + 1, title));
                    lines.push(format!("   {}", url));
                    if let Some(desc) = item["description"].as_str() {
                        lines.push(format!("   {}", desc));
                    }
                }
                ToolResult::success_text(lines.join("\n"))
            }
            _ => ToolResult::success_text(format!("No results for: {}", query)),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }
}

/// Tool to fetch and extract content from a URL
pub struct WebFetchTool {
    max_chars: usize,
}

impl WebFetchTool {
    /// Create a new WebFetchTool
    pub fn new() -> Self {
        Self { max_chars: 50000 }
    }

    /// Create with custom max characters
    pub fn with_max_chars(max_chars: usize) -> Self {
        Self { max_chars }
    }
}

impl Default for WebFetchTool {
    fn default() -> Self {
        Self { max_chars: 50000 }
    }
}

#[async_trait]
impl SimpleTool for WebFetchTool {
    fn name(&self) -> &str {
        "web_fetch"
    }

    fn description(&self) -> &str {
        "Fetch URL and extract readable content (HTML → markdown/text)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch"
                },
                "extract_mode": {
                    "type": "string",
                    "enum": ["markdown", "text"],
                    "default": "markdown"
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 100
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let url = match input.get_str("url") {
            Some(u) => u,
            None => return ToolResult::failure("Missing 'url' parameter"),
        };

        let extract_mode = input.get_str("extract_mode").unwrap_or("markdown");

        let max_chars = input
            .get_number("max_chars")
            .unwrap_or(self.max_chars as f64) as usize;

        let client = reqwest::Client::new();
        let response = match client
            .get(url)
            .header("User-Agent", USER_AGENT)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return ToolResult::failure(format!("Request failed: {}", e)),
        };

        let status = response.status();
        let final_url = response.url().clone();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body = match response.bytes().await {
            Ok(b) => b,
            Err(e) => return ToolResult::failure(format!("Failed to read response: {}", e)),
        };

        let text = String::from_utf8_lossy(&body);

        // Check content type
        let (content, extractor_type) = if content_type.contains("application/json") {
            // JSON - try to format
            if let Ok(json) = serde_json::from_str::<Value>(&text) {
                (
                    serde_json::to_string_pretty(&json).unwrap_or(text.to_string()),
                    "json",
                )
            } else {
                (text.to_string(), "raw")
            }
        } else if content_type.contains("text/html")
            || text.to_lowercase().starts_with("<!doctype")
            || text.to_lowercase().starts_with("<html")
        {
            // HTML - extract readable content
            let product = match extractor::extract(&mut text.as_bytes(), &final_url) {
                Ok(p) => p,
                Err(e) => {
                    return ToolResult::failure(format!("Readability extraction failed: {}", e));
                }
            };
            let html_content = product.content;
            let title = product.title;

            let content = if extract_mode == "markdown" {
                html_to_markdown(&html_content)
            } else {
                strip_html_tags(&html_content)
            };

            let full_content = if !title.is_empty() {
                format!("# {}\n\n{}", title, content)
            } else {
                content
            };

            (full_content, "readability")
        } else {
            // Raw text
            (text.to_string(), "raw")
        };

        let content_len = content.len();
        let truncated = content_len > max_chars;
        let truncated_content = if truncated {
            let chars = content.chars();
            chars.take(max_chars).collect::<String>()
        } else {
            content
        };

        let result = json!({
            "url": url,
            "finalUrl": final_url.as_str(),
            "status": status.as_u16(),
            "extractor": extractor_type,
            "truncated": truncated,
            "length": content_len,
            "text": truncated_content
        });

        ToolResult::success_text(match serde_json::to_string(&result) {
            Ok(s) => s,
            Err(e) => format!("{{\"error\": \"Failed to serialize result: {}\"}}", e),
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Web
    }
}

use std::sync::LazyLock;

static LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"<a\s+[^>]*href=["']([^"']+)["'][^>]*>([^<]+)</a>"#).unwrap());
static HEADING_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<h([1-6])[^>]*>([^<]+)</h\1>").unwrap());
static BOLD1_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<strong[^>]*>([^<]+)</strong>").unwrap());
static BOLD2_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<b[^>]*>([^<]+)</b>").unwrap());
static ITALIC1_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<em[^>]*>([^<]+)</em>").unwrap());
static ITALIC2_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<i[^>]*>([^<]+)</i>").unwrap());
static CODEBLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<pre[^>]*><code[^>]*>([\s\S]+?)</code></pre>").unwrap());
static INLINECODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<code[^>]*>([^<]+)</code>").unwrap());
static LIST_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<li[^>]*>([^<]+)</li>").unwrap());
static PARAGRAPH_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"</(p|div|section|article)>").unwrap());
static BREAK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<br\s*/?>").unwrap());
static SCRIPT_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<script[\s\S]*?</script>").unwrap());
static STYLE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<style[\s\S]*?</style>").unwrap());
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static WS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t]+").unwrap());
static NEWLINES_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n{3,}").unwrap());

/// Convert HTML to simplified markdown
fn html_to_markdown(html: &str) -> String {
    let mut result = html.to_string();

    // Convert links first (before other processing)
    result = LINK_RE.replace_all(&result, "[$2]($1)").to_string();

    // Convert headings
    result = HEADING_RE
        .replace_all(&result, |caps: &regex::Captures| {
            let level = caps.get(1).unwrap().as_str().parse::<usize>().unwrap_or(1);
            let text = caps.get(2).unwrap().as_str();
            format!("\n{} {}\n", "#".repeat(level), text)
        })
        .to_string();

    // Convert bold
    result = BOLD1_RE.replace_all(&result, "**$1**").to_string();
    result = BOLD2_RE.replace_all(&result, "**$1**").to_string();

    // Convert italic
    result = ITALIC1_RE.replace_all(&result, "_$1_").to_string();
    result = ITALIC2_RE.replace_all(&result, "_$1_").to_string();

    // Convert code blocks
    result = CODEBLOCK_RE.replace_all(&result, "```\n$1\n```").to_string();

    // Convert inline code
    result = INLINECODE_RE.replace_all(&result, "`$1`").to_string();

    // Convert lists
    result = LIST_RE.replace_all(&result, "\n- $1").to_string();

    // Convert paragraphs and line breaks
    result = PARAGRAPH_RE.replace_all(&result, "\n\n").to_string();
    result = BREAK_RE.replace_all(&result, "\n").to_string();

    // Strip remaining tags
    result = strip_html_tags(&result);

    // Normalize whitespace
    normalize_whitespace(&result)
}

/// Strip all HTML tags
fn strip_html_tags(html: &str) -> String {
    // Remove script and style tags first
    let html = SCRIPT_RE.replace_all(html, "");
    let html = STYLE_RE.replace_all(&html, "");

    // Remove remaining tags
    let result = TAG_RE.replace_all(&html, "");

    // Decode HTML entities
    html_escape::decode_html_entities(&result).to_string()
}

/// Normalize whitespace
fn normalize_whitespace(text: &str) -> String {
    let text = WS_RE.replace_all(text, " ");
    NEWLINES_RE.replace_all(&text, "\n\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        let html = "<p>Hello <strong>world</strong>!</p>";
        let result = strip_html_tags(html);
        assert_eq!(result, "Hello world!");
    }

    #[test]
    fn test_normalize_whitespace() {
        let text = "Hello    world\n\n\n\nTest";
        let result = normalize_whitespace(text);
        assert_eq!(result, "Hello world\n\nTest");
    }
}
