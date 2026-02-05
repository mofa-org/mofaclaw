//! Telegram channel implementation using teloxide

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::TelegramConfig;
use crate::error::{ChannelError, MofaclawError, Result};
use crate::messages::{InboundMessage, OutboundMessage};
use crate::provider::TranscriptionProvider;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use teloxide::{net::Download, prelude::*, types::ChatId, types::ParseMode};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Telegram channel using teloxide
pub struct TelegramChannel {
    config: TelegramConfig,
    bot: Arc<Bot>,
    bus: MessageBus,
    running: Arc<RwLock<bool>>,
    transcription_provider: Option<Arc<dyn TranscriptionProvider>>,
    media_dir: PathBuf,
}

impl TelegramChannel {
    /// Create a new Telegram channel
    pub fn new(config: TelegramConfig, bus: MessageBus) -> Result<Self> {
        if config.token.is_empty() {
            return Err(ChannelError::NotConfigured("Telegram".to_string()).into());
        }

        let bot = Bot::new(&config.token);

        // Create media directory
        let media_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".mofaclaw")
            .join("media");

        Ok(Self {
            config,
            bot: Arc::new(bot),
            bus,
            running: Arc::new(RwLock::new(false)),
            transcription_provider: None,
            media_dir,
        })
    }

    /// Set the transcription provider for voice messages
    pub fn with_transcription(mut self, provider: Arc<dyn TranscriptionProvider>) -> Self {
        self.transcription_provider = Some(provider);
        self
    }

    /// Convert markdown to Telegram-safe HTML
    fn markdown_to_html(&self, text: &str) -> String {
        // Simple implementation - escape HTML special characters
        // In a full implementation, this would do proper markdown conversion
        let mut result = text.to_string();

        // Escape HTML
        result = result.replace('&', "&amp;");
        result = result.replace('<', "&lt;");
        result = result.replace('>', "&gt;");

        // Bold **text** or __text__
        let re = regex::Regex::new(r"\*\*([^*]+)\*\*").unwrap();
        result = re.replace_all(&result, "<b>$1</b>").to_string();

        let re = regex::Regex::new(r"__([^_]+)__").unwrap();
        result = re.replace_all(&result, "<b>$1</b>").to_string();

        // Italic _text_
        let re = regex::Regex::new(r"(?<![a-zA-Z0-9])_([^_]+)_(?![a-zA-Z0-9])").unwrap();
        result = re.replace_all(&result, "<i>$1</i>").to_string();

        // Links [text](url)
        let re = regex::Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap();
        result = re.replace_all(&result, "<a href=\"$2\">$1</a>").to_string();

        // Headers # Title -> Title
        let re = regex::Regex::new(r"(?m)^#{1,6}\s+(.+)$").unwrap();
        result = re.replace_all(&result, "$1").to_string();

        // Code blocks
        let re = regex::Regex::new(r"```([\s\S]*?)```").unwrap();
        result = re.replace_all(&result, "<pre>$1</pre>").to_string();

        // Inline code
        let re = regex::Regex::new(r"`([^`]+)`").unwrap();
        result = re.replace_all(&result, "<code>$1</code>").to_string();

        result
    }

    /// Send an outbound message
    pub async fn send(&self, msg: &OutboundMessage) -> Result<()> {
        let chat_id = msg
            .chat_id
            .parse::<i64>()
            .map_err(|_| ChannelError::SendFailed(format!("Invalid chat_id: {}", msg.chat_id)))?;

        let html_content = self.markdown_to_html(&msg.content);

        self.bot
            .send_message(ChatId(chat_id), html_content)
            .parse_mode(ParseMode::Html)
            .await
            .map_err(|e| MofaclawError::from(ChannelError::SendFailed(format!("Telegram error: {}", e))))?;

        Ok(())
    }

    /// Download media file and return the local path
    async fn download_media(
        bot: &Bot,
        info: &MediaInfo,
        media_dir: &PathBuf,
        content_parts: &mut Vec<String>,
        media_paths: &mut Vec<String>,
        transcription_provider: Option<&dyn TranscriptionProvider>,
    ) -> Result<()> {
        // Get the file
        let file = bot.get_file(&info.file_id).await
            .map_err(|e| ChannelError::SendFailed(format!("Failed to get file: {}", e)))?;

        // Determine file extension
        let ext = Self::get_extension(&info.media_type, info.mime_type.as_deref());

        // Save to media directory
        let file_name = format!("{}{}", &info.file_id[..info.file_id.len().min(16)], ext);
        let file_path = media_dir.join(&file_name);

        // Create the file for writing
        let mut file_handle = tokio::fs::File::create(&file_path).await
            .map_err(|e| ChannelError::SendFailed(format!("Failed to create file: {}", e)))?;

        // Download the file directly to disk
        bot.download_file(&file.path, &mut file_handle).await
            .map_err(|e| ChannelError::SendFailed(format!("Failed to download file: {}", e)))?;

        let file_path_str = file_path.to_string_lossy().to_string();
        media_paths.push(file_path_str.clone());

        // Handle voice transcription
        match info.media_type {
            MediaType::Voice | MediaType::Audio => {
                if let Some(provider) = transcription_provider {
                    match provider.transcribe(&file_path).await {
                        Ok(transcription) if !transcription.is_empty() => {
                            info!("Transcribed audio: {} chars", transcription.len());
                            content_parts.push(format!("[transcription: {}]", transcription));
                        }
                        _ => {
                            content_parts.push(format!("[{}: {}]", info.media_type.as_str(), file_path_str));
                        }
                    }
                } else {
                    content_parts.push(format!("[{}: {}]", info.media_type.as_str(), file_path_str));
                }
            }
            _ => {
                content_parts.push(format!("[{}: {}]", info.media_type.as_str(), file_path_str));
            }
        }

        debug!("Downloaded {} to {}", info.media_type.as_str(), file_path_str);
        Ok(())
    }

    /// Get file extension based on media type
    fn get_extension(media_type: &MediaType, mime_type: Option<&str>) -> String {
        if let Some(mime) = mime_type {
            match mime {
                "image/jpeg" => return ".jpg".to_string(),
                "image/png" => return ".png".to_string(),
                "image/gif" => return ".gif".to_string(),
                "image/webp" => return ".webp".to_string(),
                "audio/ogg" => return ".ogg".to_string(),
                "audio/mpeg" => return ".mp3".to_string(),
                "audio/mp4" => return ".m4a".to_string(),
                "audio/wav" => return ".wav".to_string(),
                _ => {}
            }
        }

        match media_type {
            MediaType::Photo => ".jpg".to_string(),
            MediaType::Voice => ".ogg".to_string(),
            MediaType::Audio => ".mp3".to_string(),
            MediaType::Document => String::new(),
        }
    }
}

/// Media information from Telegram
struct MediaInfo {
    file_id: String,
    media_type: MediaType,
    mime_type: Option<String>,
}

/// Media type
enum MediaType {
    Photo,
    Voice,
    Audio,
    Document,
}

impl MediaType {
    fn as_str(&self) -> &'static str {
        match self {
            MediaType::Photo => "image",
            MediaType::Voice => "voice",
            MediaType::Audio => "audio",
            MediaType::Document => "file",
        }
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn start(&self) -> Result<()> {
        *self.running.write().await = true;

        let bot = Arc::clone(&self.bot);
        let running = Arc::clone(&self.running);
        let bus = self.bus.clone();
        let transcription_provider = self.transcription_provider.clone();
        let media_dir = self.media_dir.clone();
        let config = self.config.clone();

        info!("Starting Telegram bot");

        // Create media directory if it doesn't exist
        tokio::fs::create_dir_all(&media_dir).await?;

        // Clone for the move
        let bot_clone = Arc::clone(&bot);

        // Set up message handler - using teloxide's Dispatcher
        let handler = move |msg: Message, bot: Arc<Bot>| async move {
            if let Some(user) = msg.from() {
                debug!("Telegram message from {:?}", user.id.0);

                // Convert to InboundMessage
                let sender_id = if let Some(username) = &user.username {
                    format!("{}|{}", user.id.0, username)
                } else {
                    user.id.0.to_string()
                };

                let chat_id = msg.chat.id.0.to_string();

                // Build content from text and/or media
                let mut content_parts = Vec::new();
                let mut media_paths = Vec::new();

                // Text content
                if let Some(text) = msg.text() {
                    content_parts.push(text.to_string());
                }
                if let Some(caption) = msg.caption() {
                    content_parts.push(caption.to_string());
                }

                // Handle media files - use methods to access optional fields
                let media_file = msg.photo().and_then(|p| p.last())
                    .map(|ph| MediaInfo {
                        file_id: ph.file.id.clone(),
                        media_type: MediaType::Photo,
                        mime_type: Some("image/jpeg".to_string()),
                    })
                    .or_else(|| msg.voice().map(|v| MediaInfo {
                        file_id: v.file.id.clone(),
                        media_type: MediaType::Voice,
                        mime_type: v.mime_type.as_ref().map(|m| m.to_string()),
                    }))
                    .or_else(|| msg.audio().map(|a| MediaInfo {
                        file_id: a.file.id.clone(),
                        media_type: MediaType::Audio,
                        mime_type: a.mime_type.as_ref().map(|m| m.to_string()),
                    }))
                    .or_else(|| msg.document().map(|d| MediaInfo {
                        file_id: d.file.id.clone(),
                        media_type: MediaType::Document,
                        mime_type: d.mime_type.as_ref().map(|m| m.to_string()),
                    }));

                // Download media if present
                if let Some(info) = media_file {
                    if let Err(e) = Self::download_media(
                        &bot,
                        &info,
                        &media_dir,
                        &mut content_parts,
                        &mut media_paths,
                        transcription_provider.as_deref(),
                    ).await {
                        warn!("Failed to download media: {}", e);
                        content_parts.push(format!("[{}: download failed]", info.media_type.as_str()));
                    }
                }

                let content = if content_parts.is_empty() {
                    "[empty message]".to_string()
                } else {
                    content_parts.join("\n")
                };

                debug!("Telegram message from {}: {}", sender_id, content);

                // Publish to message bus
                let inbound_msg = InboundMessage::new(
                    "telegram",
                    &sender_id,
                    &chat_id,
                    &content,
                ).with_media(media_paths);

                if let Err(e) = bus.publish_inbound(inbound_msg).await {
                    error!("Failed to publish message to bus: {}", e);
                }
            }

            respond(())
        };

        // In a real implementation, we'd set up the dispatcher here
        // For now, just mark as running and log
        info!("Telegram bot started");

        // Keep the loop running
        while *running.read().await {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        *self.running.write().await = false;
        info!("Stopping Telegram bot");
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled && !self.config.token.is_empty()
    }
}
