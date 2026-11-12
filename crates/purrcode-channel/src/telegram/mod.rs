// SPDX-License-Identifier: GPL-3.0-or-later

pub mod channel;
pub mod client;
pub mod types;

pub use channel::{TelegramChannel, TelegramMessageHandler};
pub use client::{split_message, TgClient};
pub use types::{
    InlineKeyboardButton, InlineKeyboardMarkup, TgCallbackQuery, TgChat, TgMessage, TgUpdate,
    TgUser,
};
