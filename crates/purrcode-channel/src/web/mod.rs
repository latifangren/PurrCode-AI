// SPDX-License-Identifier: GPL-3.0-or-later

pub mod channel;
pub mod html;
pub mod html_css;
pub mod html_js;
pub mod html_js_b5;
pub mod server;
pub mod sessions;

pub use channel::{WebChannel, WebSkillDraftRequester, SKILL_DRAFT_KEY_PREFIX};
pub use html::web_index_html;
pub use server::{PromptDispatcher, WebServer};
pub use sessions::{
    is_valid_web_session_id, new_web_session_id, WebSession, WebSessionInfo, WebSessionRegistry,
    DEFAULT_WEB_SESSION_ID,
};
