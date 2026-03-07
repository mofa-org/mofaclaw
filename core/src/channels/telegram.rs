//! Telegram channel implementation using teloxide

use super::base::Channel;
use crate::bus::MessageBus;
use crate::config::TelegramConfig;
use crate::error::{ChannelError, MofaclawError, Result};
use crate::messages::{InboundMessage, OutboundMessage};
use crate::provider::TranscriptionProvider;
use async_trait::async_trait;
use regex::Regex;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use teloxide::{
    dispatching::Dispatcher,
    net::Download,
    prelude::*,
    types::{BotCommand, ChatAction, ChatId, ParseMode},
};
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

static BOLD1_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*\*([^*]+)\*\*").unwrap());
static BOLD2_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"__([^_]+)__").unwrap());
// Use word boundaries around _text_ to approximate the original intent
// without using unsupported look-around in Rust's regex engine.
static ITALIC_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\\b_([^_]+)_\\b").unwrap());
static LINK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
static HEADER_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?m)^#{1,6}\s+(.+)$").unwrap());
static CODEBLOCK_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"```([\s\S]*?)```").unwrap());
static INLINECODE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`([^`]+)`").unwrap());

/// Telegram channel using teloxide
pub struct TelegramChannel {
    config: TelegramConfig,
    bot: Arc<Bot>,
    bus: MessageBus,
    running: Arc<RwLock<bool>>,
    transcription_provider: Option<Arc<dyn TranscriptionProvider>>,
    media_dir: PathBuf,
    dispatcher_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
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
            dispatcher_handle: Arc::new(RwLock::new(None)),
        })
    }

    /// Set the transcription provider for voice messages
    pub fn with_transcription(mut self, provider: Arc<dyn TranscriptionProvider>) -> Self {
        self.transcription_provider = Some(provider);
        self
    }
    /// convert markdown to telegram-safe html (static version for use in closures)
    fn markdown_to_html_static(text: &str) -> String {
        let mut result = text.to_string();

        // escape html
        result = result.replace('&', "&amp;");
        result = result.replace('<', "&lt;");
        result = result.replace('>', "&gt;");

        // bold **text** or __text__
        result = BOLD1_RE.replace_all(&result, "<b>$1</b>").to_string();
        result = BOLD2_RE.replace_all(&result, "<b>$1</b>").to_string();

        // italic _text_
        result = ITALIC_RE.replace_all(&result, "<i>$1</i>").to_string();

        // links [text](url)
        result = LINK_RE
            .replace_all(&result, "<a href=\"$2\">$1</a>")
            .to_string();

        // headers # title -> title
        result = HEADER_RE.replace_all(&result, "$1").to_string();

        // code blocks
        result = CODEBLOCK_RE
            .replace_all(&result, "<pre>$1</pre>")
            .to_string();

        // inline code
        result = INLINECODE_RE
            .replace_all(&result, "<code>$1</code>")
            .to_string();

        result
    }

    /// convert markdown to telegram-safe html
    fn markdown_to_html(&self, text: &str) -> String {
        Self::markdown_to_html_static(text)
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
            .map_err(|e| {
                MofaclawError::from(ChannelError::SendFailed(format!("Telegram error: {}", e)))
            })?;

        Ok(())
    }

    /// Download media file and return the local path
    async fn download_media(
        bot: &Bot,
        info: &MediaInfo,
        media_dir: &std::path::Path,
        content_parts: &mut Vec<String>,
        media_paths: &mut Vec<String>,
        transcription_provider: Option<&dyn TranscriptionProvider>,
    ) -> Result<()> {
        // Get the file
        let file = bot
            .get_file(&info.file_id)
            .await
            .map_err(|e| ChannelError::SendFailed(format!("Failed to get file: {}", e)))?;

        // Determine file extension
        let ext = Self::get_extension(&info.media_type, info.mime_type.as_deref());

        // Save to media directory
        let file_name = format!("{}{}", &info.file_id[..info.file_id.len().min(16)], ext);
        let file_path = media_dir.join(&file_name);

        // Create the file for writing
        let mut file_handle = tokio::fs::File::create(&file_path)
            .await
            .map_err(|e| ChannelError::SendFailed(format!("Failed to create file: {}", e)))?;

        // Download the file directly to disk
        bot.download_file(&file.path, &mut file_handle)
            .await
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
                            content_parts.push(format!(
                                "[{}: {}]",
                                info.media_type.as_str(),
                                file_path_str
                            ));
                        }
                    }
                } else {
                    content_parts.push(format!(
                        "[{}: {}]",
                        info.media_type.as_str(),
                        file_path_str
                    ));
                }
            }
            _ => {
                content_parts.push(format!("[{}: {}]", info.media_type.as_str(), file_path_str));
            }
        }

        debug!(
            "Downloaded {} to {}",
            info.media_type.as_str(),
            file_path_str
        );
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::TelegramConfig;

    fn create_test_channel() -> TelegramChannel {
        let config = TelegramConfig {
            enabled: true,
            token: "test-token".to_string(),
            allow_from: Vec::new(),
        };
        let bus = MessageBus::new();
        TelegramChannel::new(config, bus).expect("failed to create TelegramChannel")
    }

    #[test]
    fn markdown_to_html_formats_basic_markdown() {
        let channel = create_test_channel();

        let input = "Hello **world** and __also bold__, _italic_ and [link](https://example.com)";
        let html = channel.markdown_to_html(input);

        // Basic HTML escaping
        assert!(!html.contains("<script"), "HTML should be escaped");

        // Bold / link conversions
        assert!(
            html.contains("<b>world</b>") && html.contains("<b>also bold</b>"),
            "bold formatting should be converted"
        );
        assert!(
            html.contains("<a href=\"https://example.com\">link</a>"),
            "links should be converted"
        );
    }

    #[test]
    fn media_type_as_str_and_extension_are_consistent() {
        // Photo
        assert_eq!(MediaType::Photo.as_str(), "image");
        assert_eq!(
            TelegramChannel::get_extension(&MediaType::Photo, None),
            ".jpg".to_string()
        );

        // Voice
        assert_eq!(MediaType::Voice.as_str(), "voice");
        assert_eq!(
            TelegramChannel::get_extension(&MediaType::Voice, Some("audio/ogg")),
            ".ogg".to_string()
        );

        // Audio
        assert_eq!(MediaType::Audio.as_str(), "audio");
        assert_eq!(
            TelegramChannel::get_extension(&MediaType::Audio, Some("audio/mpeg")),
            ".mp3".to_string()
        );

        // Document
        assert_eq!(MediaType::Document.as_str(), "file");
        assert_eq!(
            TelegramChannel::get_extension(&MediaType::Document, None),
            String::new()
        );
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
        let allow_from = self.config.allow_from.clone();

        info!("Starting Telegram bot");

        // Create media directory if it doesn't exist
        tokio::fs::create_dir_all(&media_dir).await?;

        // Set up bot commands menu
        let commands = vec![
            BotCommand::new("start", "Show welcome message"),
            BotCommand::new("help", "Show help information"),
        ];
        if let Err(e) = bot.set_my_commands(commands).await {
            warn!("Failed to set bot commands menu: {}", e);
        }

        // Set up message handler function
        let handler = move |msg: Message, bot: Bot| {
            let bus = bus.clone();
            let transcription_provider = transcription_provider.clone();
            let media_dir = media_dir.clone();
            let allow_from = allow_from.clone();

            async move {
                // Get user info - skip if missing or bot
                let user = match msg.from.as_ref() {
                    Some(u) if u.is_bot => {
                        debug!("Ignoring message from bot user {:?}", u.id.0);
                        return respond(());
                    }
                    Some(u) => u,
                    None => {
                        debug!("Message without user info, skipping");
                        return respond(());
                    }
                };

                // Check allow_from list (security: enforce access control)
                let user_id_str = user.id.0.to_string();
                let username = user.username.as_deref();
                if !allow_from.is_empty() {
                    let allowed = allow_from.contains(&user_id_str)
                        || username
                            .map(|u| allow_from.contains(&u.to_string()))
                            .unwrap_or(false);
                    if !allowed {
                        debug!(
                            "Ignoring message from unauthorized user: {} ({})",
                            user_id_str,
                            username.unwrap_or("no username")
                        );
                        return respond(());
                    }
                }

                debug!("Telegram message from {:?}", user.id.0);

                let chat_id = msg.chat.id.0.to_string();

                // Show typing indicator while processing
                let _ = bot
                    .send_chat_action(ChatId(msg.chat.id.0), ChatAction::Typing)
                    .await;

                // Handle commands - parse properly to handle /start foo and /help@botname
                if let Some(text) = msg.text() {
                    // Parse command (handles /command@botname and /command args)
                    let cmd = if let Some(cmd_text) = text.strip_prefix('/') {
                        let end = cmd_text
                            .find(' ')
                            .or_else(|| cmd_text.find('@'))
                            .unwrap_or(cmd_text.len());
                        Some(cmd_text[..end].to_lowercase())
                    } else {
                        None
                    };
                    if let Some(cmd) = cmd {
                        match cmd.as_str() {
                            "start" => {
                                let welcome_msg = "👋 Welcome to mofaclaw!\n\n\
                                    I'm your agent assistant. Just send me a message and I'll process it.\n\n\
                                    Use /help for more information.";
                                if let Err(e) =
                                    bot.send_message(ChatId(msg.chat.id.0), welcome_msg).await
                                {
                                    error!("Failed to send welcome message: {}", e);
                                }
                                return respond(());
                            }
                            "help" => {
                                let help_markdown = "📖 **Help**\n\n\
                                        **How to use:**\n\
                                        Just send me any message and I'll forward it to the mofaclaw agent for processing.\n\n\
                                        **Supported:**\n\
                                        • Text messages\n\
                                        • Photos with captions\n\
                                        • Voice messages (transcribed if available)\n\
                                        • Audio files\n\
                                        • Documents\n\n\
                                        **Commands:**\n\
                                        /start - Show welcome message\n\
                                        /help - Show this help message\n\n\
                                        Responses will be sent back to this chat.";
                                // Convert markdown to HTML for reliable parsing
                                let help_html = Self::markdown_to_html_static(help_markdown);
                                if let Err(e) = bot
                                    .send_message(ChatId(msg.chat.id.0), help_html)
                                    .parse_mode(ParseMode::Html)
                                    .await
                                {
                                    error!("Failed to send help message: {}", e);
                                }
                                return respond(());
                            }
                            _ => {
                                // Unknown command, continue to process as regular message
                            }
                        }
                    }
                }

                // Convert to InboundMessage
                let sender_id = if let Some(username) = &user.username {
                    format!("{}|{}", user.id.0, username)
                } else {
                    user.id.0.to_string()
                };

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
                let media_file = msg
                    .photo()
                    .and_then(|p| p.last())
                    .map(|ph| MediaInfo {
                        file_id: ph.file.id.clone(),
                        media_type: MediaType::Photo,
                        mime_type: Some("image/jpeg".to_string()),
                    })
                    .or_else(|| {
                        msg.voice().map(|v| MediaInfo {
                            file_id: v.file.id.clone(),
                            media_type: MediaType::Voice,
                            mime_type: v.mime_type.as_ref().map(|m| m.to_string()),
                        })
                    })
                    .or_else(|| {
                        msg.audio().map(|a| MediaInfo {
                            file_id: a.file.id.clone(),
                            media_type: MediaType::Audio,
                            mime_type: a.mime_type.as_ref().map(|m| m.to_string()),
                        })
                    })
                    .or_else(|| {
                        msg.document().map(|d| MediaInfo {
                            file_id: d.file.id.clone(),
                            media_type: MediaType::Document,
                            mime_type: d.mime_type.as_ref().map(|m| m.to_string()),
                        })
                    });

                // Download media if present
                if let Some(info) = media_file
                    && let Err(e) = Self::download_media(
                        &bot,
                        &info,
                        &media_dir,
                        &mut content_parts,
                        &mut media_paths,
                        transcription_provider.as_deref(),
                    )
                    .await
                {
                    warn!("Failed to download media: {}", e);
                    content_parts.push(format!("[{}: download failed]", info.media_type.as_str()));
                }

                let content = if content_parts.is_empty() {
                    "[empty message]".to_string()
                } else {
                    content_parts.join("\n")
                };

                // Better empty message handling - provide user feedback
                if content == "[empty message]" && media_paths.is_empty() {
                    debug!(
                        "Received empty message from {} in chat {}",
                        sender_id, chat_id
                    );
                    let empty_msg = "I didn't receive any content in your message.\n\n\
                        Please send:\n\
                        • Text message\n\
                        • Photo with caption\n\
                        • Voice/audio message\n\
                        • Document\n\n\
                        Use /help for more information.";
                    if let Err(e) = bot.send_message(ChatId(msg.chat.id.0), empty_msg).await {
                        error!("Failed to send empty message feedback: {}", e);
                    }
                    return respond(());
                }

                debug!("Telegram message from {}: {}", sender_id, content);

                // Publish to message bus
                let inbound_msg = InboundMessage::new("telegram", &sender_id, &chat_id, &content)
                    .with_media(media_paths);

                if let Err(e) = bus.publish_inbound(inbound_msg).await {
                    error!("Failed to publish message to bus: {}", e);
                    // Send user-friendly error feedback
                    let error_msg = "Sorry, I encountered an error processing your message. Please try again later.";
                    if let Err(e2) = bot.send_message(ChatId(msg.chat.id.0), error_msg).await {
                        error!("Failed to send error feedback: {}", e2);
                    }
                }

                respond(())
            }
        };

        // Set up dispatcher with message handler
        let mut dispatcher = Dispatcher::builder(
            bot.as_ref().clone(),
            Update::filter_message().endpoint(handler),
        )
        .build();

        // Spawn dispatcher task
        let running_clone = Arc::clone(&running);
        let dispatcher_handle = tokio::spawn(async move {
            info!("Telegram dispatcher started");
            dispatcher.dispatch().await;
            info!("Telegram dispatcher stopped");
        });

        // Store dispatcher handle for graceful shutdown
        *self.dispatcher_handle.write().await = Some(dispatcher_handle);

        info!("Telegram bot started and listening for messages");

        // Wait for stop signal
        while *running_clone.read().await {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        *self.running.write().await = false;

        // Cancel dispatcher task gracefully and wait for it to finish
        let mut handle_guard = self.dispatcher_handle.write().await;
        if let Some(handle) = handle_guard.take() {
            // Request cancellation and wait for the dispatcher task to finish
            handle.abort();
            match handle.await {
                Ok(_) => {
                    // Dispatcher finished normally after abort request
                    debug!("Telegram dispatcher task finished");
                }
                Err(e) if e.is_cancelled() => {
                    // Expected cancellation error after abort; ignore
                    debug!("Telegram dispatcher task cancelled");
                }
                Err(e) => {
                    // Log unexpected join error
                    error!("Telegram dispatcher task join error: {}", e);
                }
            }
            info!("Telegram dispatcher task stopped");
        }

        info!("Stopping Telegram bot");
        Ok(())
    }

    fn is_enabled(&self) -> bool {
        self.config.enabled && !self.config.token.is_empty()
    }
}
