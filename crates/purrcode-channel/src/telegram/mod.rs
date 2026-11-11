// SPDX-License-Identifier: GPL-3.0-or-later

//! Telegram channel transport (v0.2, ADR-013).
//!
//! Hand-rolled Bot API client + poll loop + per-chat sessions + inline
//! approval, built on the shared `Channel` trait. No Telegram SDK.

pub mod approval;
pub mod channel;
pub mod chunk;
pub mod client;
pub mod render;
pub mod sessions;
pub mod types;

pub use approval::{TelegramApprovalRequester, TelegramAskRequester};
pub use channel::{telegram_state_dir, InboundMessage, TelegramChannel, TelegramChatChannel};
pub use client::{TelegramApiClient, TelegramError};
pub use sessions::ChatSessionIndex;
pub use types::{BotInfo, CallbackQuery, Chat, Message, Update, User};
