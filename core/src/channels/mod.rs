//! Channel implementations for messaging platforms

pub mod base;
pub mod discord;
pub mod dingtalk;
pub mod feishu;
pub mod manager;
pub mod telegram;
pub mod whatsapp;

pub use base::Channel;
pub use discord::DiscordChannel;
pub use dingtalk::DingTalkChannel;
pub use feishu::FeishuChannel;
pub use manager::ChannelManager;
pub use telegram::TelegramChannel;
pub use whatsapp::WhatsAppChannel;
