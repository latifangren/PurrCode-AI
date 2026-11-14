// SPDX-License-Identifier: GPL-3.0-or-later

//! Heartbeat / proactive (v0.3 F22, ADR-014 item 6): a periodic tick
//! in the gateway daemon that reads the user's `HEARTBEAT.md` task
//! list, asks the model (two-phase: decide, then run only what is
//! worth running), and delivers one line per item that produced
//! something — through the same delivery queue cron uses — to the
//! last active Telegram chat, or queue-only for the next TUI open.
//! The `purrcode heartbeat` CLI lives in purrcode-cli
//! (`heartbeat_cli.rs`) and calls into this module.

pub mod prefs;
pub mod runner;
pub mod tasks;

pub use runner::HeartbeatRunner;
