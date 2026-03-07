//! Videolizer tool: invokes the Python videolizer to generate full shorts.
//!
//! Runs `python -m videolizer full --plan plan.json --out out_dir` as a subprocess.
//! Returns job path and result JSON to the caller.

use super::base::{SimpleTool, ToolInput, ToolResult};
use async_trait::async_trait;
use mofa_sdk::agent::ToolCategory;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::fs;
use tokio::process::Command;
use tokio::time::timeout;

/// Tool to generate a full video short via the Videolizer Python pipeline.
pub struct VideolizeTool {
    /// Workspace path for job directories
    workspace: PathBuf,
    /// Path to videolizer (third_party/videolizer or VIDEOLIZER_PATH)
    videolizer_path: PathBuf,
    /// Timeout in seconds (video generation can take minutes)
    timeout_secs: u64,
}

impl VideolizeTool {
    /// Create a new VideolizeTool.
    pub fn new(workspace: impl AsRef<Path>) -> Self {
        let workspace = workspace.as_ref().to_path_buf();
        let videolizer_path = Self::resolve_videolizer_path(&workspace);
        Self {
            workspace,
            videolizer_path,
            timeout_secs: 600, // 10 minutes for full pipeline
        }
    }

    /// Resolve videolizer path: VIDEOLIZER_PATH env, or third_party/videolizer relative to exe.
    fn resolve_videolizer_path(workspace: &Path) -> PathBuf {
        if let Ok(p) = std::env::var("VIDEOLIZER_PATH") {
            return PathBuf::from(p);
        }
        // Try relative to executable (for cargo run: target/debug/mofaclaw -> repo root)
        if let Ok(exe) = std::env::current_exe() {
            if let Some(target) = exe.parent() {
                if let Some(repo_root) = target.parent().and_then(|p| p.parent()) {
                    let candidate = repo_root.join("third_party").join("videolizer");
                    if candidate.exists() {
                        return candidate;
                    }
                }
            }
        }
        // Fallback: workspace/../third_party/videolizer (when workspace is in repo)
        workspace
            .parent()
            .map(|p| p.join("third_party").join("videolizer"))
            .unwrap_or_else(|| PathBuf::from("third_party/videolizer"))
    }
}

#[async_trait]
impl SimpleTool for VideolizeTool {
    fn name(&self) -> &str {
        "videolize_full"
    }

    fn description(&self) -> &str {
        "Generate a full video short from character and series. Uses the Videolizer pipeline \
         (script, voiceover, images, subtitles, music). Returns job path and result. \
         Requires videolizer at third_party/videolizer or VIDEOLIZER_PATH."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "character": {
                    "type": "string",
                    "description": "Character name (e.g. Walter White)"
                },
                "series": {
                    "type": "string",
                    "description": "Series or movie name (e.g. Breaking Bad)"
                },
                "script": {
                    "type": "string",
                    "description": "Optional pre-generated narration script. If provided, videolizer will skip content generation."
                },
                "style_preset": {
                    "type": "string",
                    "description": "Optional style preset (e.g. calm, energetic)"
                }
            },
            "required": ["character", "series"]
        })
    }

    async fn execute(&self, input: ToolInput) -> ToolResult {
        let character = match input.get_str("character") {
            Some(c) => c.to_string(),
            None => return ToolResult::failure("Missing 'character' parameter"),
        };
        let series = match input.get_str("series") {
            Some(s) => s.to_string(),
            None => return ToolResult::failure("Missing 'series' parameter"),
        };
        let script = input.get_str("script").map(|s| s.to_string());

        let job_id = format!(
            "vid_{}_{}",
            character.replace(' ', "_").to_lowercase(),
            series.replace(' ', "_").to_lowercase()
        );
        let jobs_base = self.workspace.join("videolizer_jobs");
        let job_dir = jobs_base.join(&job_id);
        if let Err(e) = fs::create_dir_all(&job_dir).await {
            return ToolResult::failure(format!("Failed to create job dir: {}", e));
        }

        let plan = json!({
            "job_id": job_id,
            "character": character,
            "series": series,
            "script": script,
            "format": "video_short",
            "output": { "width": 1080, "height": 1920, "fps": 24 }
        });
        let plan_path = job_dir.join("plan.json");
        if let Err(e) = fs::write(&plan_path, plan.to_string()).await {
            return ToolResult::failure(format!("Failed to write plan: {}", e));
        }

        let python = std::env::var("VIDEOLIZER_PYTHON")
            .unwrap_or_else(|_| "python3".to_string());
        let mut cmd = Command::new(&python);
        cmd.arg("-m")
            .arg("videolizer")
            .arg("full")
            .arg("--plan")
            .arg(&plan_path)
            .arg("--out")
            .arg(&job_dir)
            .current_dir(&self.videolizer_path)
            .env("PYTHONPATH", &self.videolizer_path);

        let result = timeout(
            std::time::Duration::from_secs(self.timeout_secs),
            cmd.output(),
        )
        .await;

        let output = match result {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => {
                return ToolResult::failure(format!("Videolizer subprocess failed: {}", e));
            }
            Err(_) => {
                return ToolResult::failure(format!(
                    "Videolizer timed out after {} seconds",
                    self.timeout_secs
                ));
            }
        };

        let result_path = job_dir.join("result.json");
        let result_json = match fs::read_to_string(&result_path).await {
            Ok(s) => s,
            Err(_) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return ToolResult::failure(format!(
                    "Videolizer failed (no result.json). stderr: {}",
                    stderr
                ));
            }
        };

        let status = serde_json::from_str::<Value>(&result_json)
            .ok()
            .and_then(|v| v.get("status").and_then(|s| s.as_str().map(String::from)))
            .unwrap_or_else(|| "unknown".to_string());

        let summary = if status == "success" {
            format!(
                "Job {} completed. Artifacts in: {}",
                job_id,
                job_dir.display()
            )
        } else {
            let err = serde_json::from_str::<Value>(&result_json)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str().map(String::from)))
                .unwrap_or_else(|| "unknown error".to_string());
            format!("Job {} failed: {}. Logs: {}/logs.txt", job_id, err, job_dir.display())
        };

        ToolResult::success_text(format!(
            "{}\n\nResult JSON:\n{}",
            summary, result_json
        ))
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Custom
    }
}
