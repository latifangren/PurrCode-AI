// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron jobs (v0.3 F19, ADR-014): natural-language schedules, a job store,
//! a durable delivery queue, and the `purrcode cron` management CLI.
//!
//! The daemon tick (gateway) and the job runner land in a follow-up batch;
//! this module is the management surface + the pure scheduling core.

pub mod cli;
pub mod queue;
pub mod schedule;
pub mod store;

pub use queue::DeliveryQueue;
pub use schedule::parse_natural;
pub use store::{cron_dir, CronJob, CronStore, DeliveryTarget, JobSchedule};
