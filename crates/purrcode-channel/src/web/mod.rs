// SPDX-License-Identifier: GPL-3.0-or-later

pub mod channel;
pub mod html;
pub mod server;

pub use channel::WebChannel;
pub use html::WEB_INDEX_HTML;
pub use server::{PromptDispatcher, WebServer};
