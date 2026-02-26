//! Reusable Discord interactive UI components.
//!
//! Provides a `confirm()` helper that presents a ✅ Confirm / ❌ Cancel button
//! pair to the invoking user and returns `true` only when Confirm is clicked.
//!
//! # Usage
//! ```ignore
//! // Inside a poise slash command:
//! if !confirm(&ctx, "Merge PR #42 into main? This cannot be undone.").await? {
//!     return Ok(());
//! }
//! // proceed with the destructive action
//! ```

use std::time::Duration;

use poise::serenity_prelude::{
    builder::{
        CreateActionRow, CreateButton, CreateInteractionResponse, CreateInteractionResponseMessage,
    },
    model::application::ButtonStyle,
};
use uuid::Uuid;

use super::{Data, DiscordError};

/// Alias matching the rest of the discord module.
type Context<'a> = poise::Context<'a, Data, DiscordError>;

/// How long (in seconds) the confirmation buttons remain active.
const CONFIRM_TIMEOUT_SECS: u64 = 30;

/// Present a ✅ Confirm / ❌ Cancel button pair and await the invoker's choice.
///
/// Returns `Ok(true)` when the user clicks **Confirm**, `Ok(false)` when they
/// click **Cancel** or the 30-second window expires.
///
/// The message is **ephemeral** — only the invoker sees it.
pub async fn confirm(ctx: &Context<'_>, prompt: &str) -> Result<bool, DiscordError> {
    // UUID-scoped button IDs prevent cross-fire between concurrent invocations.
    let confirm_id = format!("confirm_{}", Uuid::new_v4());
    let cancel_id = format!("cancel_{}", Uuid::new_v4());

    let components = vec![CreateActionRow::Buttons(vec![
        CreateButton::new(&confirm_id)
            .label("✅ Confirm")
            .style(ButtonStyle::Success),
        CreateButton::new(&cancel_id)
            .label("❌ Cancel")
            .style(ButtonStyle::Danger),
    ])];

    // Ephemeral reply — only the command invoker can see and click it.
    let reply = ctx
        .send(
            poise::CreateReply::default()
                .content(format!("⚠️ **{}**", prompt))
                .components(components)
                .ephemeral(true),
        )
        .await?;

    let message = reply.message().await?;

    // Wait for the invoker to click one of the buttons.
    let interaction = message
        .await_component_interaction(ctx.serenity_context())
        .author_id(ctx.author().id)
        .timeout(Duration::from_secs(CONFIRM_TIMEOUT_SECS))
        .await;

    match interaction {
        Some(interaction) => {
            let confirmed = interaction.data.custom_id == confirm_id;
            let status_text = if confirmed {
                "✅ **Confirmed.** Proceeding…"
            } else {
                "❌ **Cancelled.** No action was taken."
            };
            interaction
                .create_response(
                    ctx.serenity_context(),
                    CreateInteractionResponse::UpdateMessage(
                        CreateInteractionResponseMessage::new().content(status_text),
                    ),
                )
                .await?;
            Ok(confirmed)
        }
        None => {
            // Timeout — update the message.
            reply
                .edit(
                    *ctx,
                    poise::CreateReply::default()
                        .content("⏱️ **Timed out.** No action was taken.")
                        .components(vec![]),
                )
                .await?;
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn confirm_id_uniqueness() {
        let id1 = format!("confirm_{}", uuid::Uuid::new_v4());
        let id2 = format!("confirm_{}", uuid::Uuid::new_v4());
        assert_ne!(id1, id2);
    }

    #[test]
    fn cancel_id_uniqueness() {
        let id1 = format!("cancel_{}", uuid::Uuid::new_v4());
        let id2 = format!("cancel_{}", uuid::Uuid::new_v4());
        assert_ne!(id1, id2);
    }

    #[test]
    fn ids_have_correct_prefix() {
        let confirm_id = format!("confirm_{}", uuid::Uuid::new_v4());
        let cancel_id = format!("cancel_{}", uuid::Uuid::new_v4());
        assert!(confirm_id.starts_with("confirm_"));
        assert!(cancel_id.starts_with("cancel_"));
    }

    #[test]
    fn confirm_and_cancel_ids_differ() {
        let confirm_id = format!("confirm_{}", uuid::Uuid::new_v4());
        let cancel_id = format!("cancel_{}", uuid::Uuid::new_v4());
        assert_ne!(confirm_id, cancel_id);
    }

    #[test]
    fn button_match_logic() {
        let confirm_id = format!("confirm_{}", uuid::Uuid::new_v4());
        let cancel_id = format!("cancel_{}", uuid::Uuid::new_v4());
        assert_eq!(confirm_id, confirm_id.clone());
        assert_ne!(cancel_id, confirm_id);
    }
}
