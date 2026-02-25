//! File system tools: read, write, edit, list

use super::base::{SimpleTool, ToolInput, ToolResult};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use tokio::fs;

/// Tool to read file contents
pub struct ReadFileTool;

impl ReadFileTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SimpleTool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read the contents of a file at the given path."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to read"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let path = match input.get_str("path") {
            Some(p) => p,
            None => return ToolResult::failure("Missing 'path' parameter"),
        };

        let path = expand_tilde(Path::new(path));

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return ToolResult::failure(format!("Error: File not found: {}", path.display()));
        }

        if !tokio::fs::metadata(&path)
            .await
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            return ToolResult::failure(format!("Error: Not a file: {}", path.display()));
        }

        match fs::read_to_string(&path).await {
            Ok(content) => ToolResult::success_text(content),
            Err(e) => ToolResult::failure(format!("Error reading file: {}", e).to_string()),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::File
    }
}

/// Tool to write content to a file
pub struct WriteFileTool;

impl WriteFileTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SimpleTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write content to a file at the given path. Creates parent directories if needed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to write to"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let path = match input.get_str("path") {
            Some(p) => p,
            None => return ToolResult::failure("Missing 'path' parameter"),
        };

        let content = match input.get_str("content") {
            Some(c) => c,
            None => return ToolResult::failure("Missing 'content' parameter"),
        };

        let path = expand_tilde(Path::new(path));

        // Create parent directories
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent).await {
                return ToolResult::failure(format!("Error creating directory: {}", e).to_string());
            }
        }

        match fs::write(&path, content).await {
            Ok(_) => ToolResult::success_text(format!(
                "Successfully wrote {} bytes to {}",
                content.len(),
                path.display()
            )),
            Err(e) => ToolResult::failure(format!("Error writing file: {}", e).to_string()),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::File
    }
}

/// Tool to edit a file by replacing text
pub struct EditFileTool;

impl EditFileTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SimpleTool for EditFileTool {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing old_text with new_text. The old_text must exist exactly in the file."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "The exact text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "The text to replace with"
                }
            },
            "required": ["path", "old_text", "new_text"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let path = match input.get_str("path") {
            Some(p) => p,
            None => return ToolResult::failure("Missing 'path' parameter"),
        };

        let old_text = match input.get_str("old_text") {
            Some(t) => t,
            None => return ToolResult::failure("Missing 'old_text' parameter"),
        };

        let new_text = match input.get_str("new_text") {
            Some(t) => t,
            None => return ToolResult::failure("Missing 'new_text' parameter"),
        };

        let path = expand_tilde(Path::new(path));

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return ToolResult::failure(format!("Error: File not found: {}", path.display()));
        }

        let content = match fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::failure(format!("Error reading file: {}", e).to_string()),
        };

        if !content.contains(old_text) {
            return ToolResult::failure(
                "Error: old_text not found in file. Make sure it matches exactly.".to_string(),
            );
        }

        let count = content.matches(old_text).count();
        if count > 1 {
            return ToolResult::failure(format!(
                "Warning: old_text appears {} times. Please provide more context to make it unique.",
                count
            ));
        }

        let new_content = content.replacen(old_text, new_text, 1);

        match fs::write(&path, new_content).await {
            Ok(_) => ToolResult::success_text(format!("Successfully edited {}", path.display())),
            Err(e) => ToolResult::failure(format!("Error writing file: {}", e).to_string()),
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::File
    }
}

/// Tool to list directory contents
pub struct ListDirTool;

impl ListDirTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl SimpleTool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn description(&self) -> &str {
        "List the contents of a directory."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The directory path to list"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let path = match input.get_str("path") {
            Some(p) => p,
            None => return ToolResult::failure("Missing 'path' parameter"),
        };

        let path = expand_tilde(Path::new(path));

        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return ToolResult::failure(format!("Error: Directory not found: {}", path.display()));
        }

        if !tokio::fs::metadata(&path)
            .await
            .map(|m| m.is_dir())
            .unwrap_or(false)
        {
            return ToolResult::failure(format!("Error: Not a directory: {}", path.display()));
        }

        let mut entries = match fs::read_dir(&path).await {
            Ok(e) => e,
            Err(e) => {
                return ToolResult::failure(format!("Error listing directory: {}", e).to_string());
            }
        };

        let mut items = Vec::new();
        loop {
            let entry = match entries.next_entry().await {
                Ok(e) => e,
                Err(err) => {
                    return ToolResult::failure(format!("Error reading directory: {}", err));
                }
            };
            let entry = match entry {
                Some(e) => e,
                None => break,
            };
            let name = entry.file_name().to_string_lossy().to_string();
            
            // Fast path: use DirEntry::file_type which doesn't require an extra stat call on most platforms
            let is_dir = if let Ok(file_type) = entry.file_type().await {
                file_type.is_dir()
            } else {
                // Fallback to metadata if file_type fails
                tokio::fs::metadata(entry.path())
                    .await
                    .map(|m| m.is_dir())
                    .unwrap_or(false)
            };

            let prefix = if is_dir {
                "📁 "
            } else {
                "📄 "
            };
            items.push(format!("{}{}", prefix, name));
        }

        if items.is_empty() {
            return ToolResult::success_text(format!("Directory {} is empty", path.display()));
        }

        items.sort();
        ToolResult::success_text(items.join("\n"))
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::File
    }
}

/// Expand tilde in path to home directory
fn expand_tilde(path: &Path) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(&path.as_os_str().to_string_lossy()[2..]);
        }
    } else if path == Path::new("~") {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_expand_tilde() {
        let expanded = expand_tilde(Path::new("~/test"));
        // Should not start with ~ anymore
        assert!(!expanded.starts_with("~"));
    }
}
