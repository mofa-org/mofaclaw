//! Channel implementations for messaging platforms

pub mod base;
pub mod manager;
pub mod telegram;
pub mod whatsapp;
pub mod dingtalk;
pub mod feishu;

pub use base::Channel;
pub use manager::ChannelManager;
pub use telegram::TelegramChannel;
pub use whatsapp::WhatsAppChannel;
pub use dingtalk::DingTalkChannel;
pub use feishu::FeishuChannel;
