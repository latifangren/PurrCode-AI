// SPDX-License-Identifier: GPL-3.0-or-later

//! The skill curator (v0.3 F20, ADR-014 item 7): a post-turn hook
//! that drafts a skill from a turn's tool-call log, stages it
//! outside the live skill dirs, and — only on the user's approval —
//! lets Tinker save it with `manage_skill action=create`.

pub mod counter;
pub mod draft;
pub mod runner;
pub mod stage;

pub use runner::{save_prompt_for, CuratorDeps, CuratorOutcome, CuratorRunner};
pub use stage::discard_draft;
