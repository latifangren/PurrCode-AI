// SPDX-License-Identifier: GPL-3.0-or-later

//! Session target-model picking for the background runners: an
//! explicit override, else the configured `model`, else a
//! deterministic scan of the configured providers.

use purrcode_api::error::{PurrError, Result};
use purrcode_config::Config;

/// Pick the session target model string: an explicit override, else the
/// configured `model`, else a deterministic scan of the configured
/// providers (lexicographic, preferring a `primary`/`default`/`main`
/// candidate). Moved here from `one_shot::resolve_target_model_or_exit`
/// in the R2 service extraction; unlike the CLI wrapper it returns an
/// error instead of exiting the process, so a background tick with no
/// model configured fails soft (tick error boundary) instead of
/// killing the daemon.
pub fn resolve_target_model(config: &Config, model_override: Option<String>) -> Result<String> {
    match model_override.or_else(|| config.model.clone()) {
        Some(m) => Ok(m),
        None => {
            // Collect all candidates deterministically
            let mut candidates: Vec<String> = Vec::new();
            let mut sorted_providers: Vec<_> = config.providers.iter().collect();
            sorted_providers.sort_by_key(|(name, _)| (*name).clone());

            for (p_name, p_cfg) in sorted_providers {
                let mut sorted_models: Vec<_> = p_cfg.models.keys().collect();
                sorted_models.sort();
                for m_key in sorted_models {
                    candidates.push(format!("{}:{}", p_name, m_key));
                }
            }

            // Prefer candidates marked primary, default, or main
            candidates
                .iter()
                .find(|c| {
                    let lower = c.to_lowercase();
                    lower.contains("primary") || lower.contains("default") || lower.contains("main")
                })
                .or_else(|| candidates.first())
                .cloned()
                .ok_or_else(|| {
                    PurrError::Config(
                        "No model configured. PurrCode AI has no hardcoded default models. \
                         Please configure your provider in '~/.purrcode/purrcode.jsonc' or run 'purrcode-ai onboard'."
                            .to_string(),
                    )
                })
        }
    }
}
