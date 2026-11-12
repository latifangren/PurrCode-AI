// SPDX-License-Identifier: GPL-3.0-or-later

//! Cron jobs (v0.3 F19, ADR-014): natural-language schedules, a job store,
//! a durable delivery queue, the `purrcode cron` management CLI, and the
//! job runner + gateway tick (batch 3b). Jobs are created via `purrcode
//! cron add`; the gateway fires them on its own tick, inside its own
//! error boundary, so a broken job can never kill the Telegram poll.

pub mod cli;
pub mod queue;
pub mod runner;
pub mod schedule;
pub mod store;

pub use queue::DeliveryQueue;
pub use runner::{build_runner_deps, CronRunner, CronRunnerDeps};
pub use schedule::parse_natural;
pub use store::{cron_dir, CronJob, CronStore, DeliveryTarget, JobSchedule};
