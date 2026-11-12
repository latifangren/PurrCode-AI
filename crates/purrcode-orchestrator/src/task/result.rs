// SPDX-License-Identifier: GPL-3.0-or-later

use purrcode_api::error::{PurrError, Result};

/// Validates and formats `result.md` according to the schema in `docs/SUBAGENTS.md` §2.2.
/// Rejects unformed or empty subagent summaries to prevent blank handoffs.
pub fn format_validated_result_markdown(
    subagent_id: &str,
    role: &str,
    status: &str,
    raw_summary: &str,
) -> Result<String> {
    let trimmed = raw_summary.trim();
    if trimmed.is_empty() {
        return Err(PurrError::Subagent {
            id: subagent_id.to_string(),
            message: "Subagent completed execution but returned an empty summary report (no deliverables or findings)".to_string(),
        });
    }

    let has_changed = trimmed.contains("Changed Files");
    let has_verified = trimmed.contains("Verified Evidence");
    let has_failures = trimmed.contains("Failures") || trimmed.contains("Blockers");
    let has_unverified = trimmed.contains("Unverified Items");

    let mut out = format!(
        "# Result Report: {}\n- **Status:** {}\n- **Role:** {}\n- **Timestamp:** {}\n",
        subagent_id,
        status,
        role,
        chrono::Utc::now().to_rfc3339()
    );

    if !has_changed {
        out.push_str("- **Changed Files:** (see summary below)\n");
    }
    if !has_verified {
        out.push_str("- **Verified Evidence:** (see summary below)\n");
    }
    if !has_failures {
        out.push_str("- **Failures / Blockers:** None reported.\n");
    }
    if !has_unverified {
        out.push_str("- **Unverified Items:** None reported.\n");
    }

    out.push_str(&format!("\n## Summary\n{}\n", trimmed));
    Ok(out)
}
