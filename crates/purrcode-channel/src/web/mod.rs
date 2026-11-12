// SPDX-License-Identifier: GPL-3.0-or-later

pub mod channel;
pub mod html;
pub mod server;

pub use channel::{WebChannel, WebSkillDraftRequester, SKILL_DRAFT_KEY_PREFIX};
pub use html::WEB_INDEX_HTML;
pub use server::{PromptDispatcher, WebServer};
