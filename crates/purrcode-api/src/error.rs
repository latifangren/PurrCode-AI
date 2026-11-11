// SPDX-License-Identifier: GPL-3.0-or-later

use thiserror::Error;

/// Core PurrCode error enumeration
#[derive(Debug, Error)]
pub enum PurrError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Provider error ({provider}): {message}")]
    Provider { provider: String, message: String },

    #[error("Provider policy refusal by {provider}: {reason}")]
    ProviderRefusal { provider: String, reason: String },

    #[error("Tool execution failed ({tool}): {message}")]
    Tool { tool: String, message: String },

    #[error("Tool not in preset: tool '{tool}' is not permitted for agent '{agent}'")]
    ToolNotInPreset { tool: String, agent: String },

    #[error("Action denied by approval rule: {0}")]
    ApprovalDenied(String),

    #[error("Memory error: {0}")]
    Memory(String),

    #[error("Session error: {0}")]
    Session(String),

    #[error("Subagent error ({id}): {message}")]
    Subagent { id: String, message: String },

    #[error("Plugin error ({plugin}): {message}")]
    Plugin { plugin: String, message: String },

    #[error("Channel error ({channel}): {message}")]
    Channel { channel: String, message: String },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal runtime error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, PurrError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_purr_error_formatting() {
        let err = PurrError::ToolNotInPreset {
            tool: "bash".to_string(),
            agent: "meowstro".to_string(),
        };
        assert_eq!(
            err.to_string(),
            "Tool not in preset: tool 'bash' is not permitted for agent 'meowstro'"
        );

        let io_err = PurrError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        ));
        assert!(io_err.to_string().contains("file missing"));
    }
}
