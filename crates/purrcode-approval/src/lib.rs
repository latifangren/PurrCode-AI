use purrcode_api::tool::ApprovalTier;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Allow,
    Ask,
    Deny,
}

pub struct ApprovalGate {
    pub mode: String,
}

impl ApprovalGate {
    pub fn new(mode: impl Into<String>) -> Self {
        Self { mode: mode.into() }
    }

    pub fn evaluate(&self, tier: ApprovalTier, _is_outside_workspace: bool) -> ApprovalDecision {
        match self.mode.as_str() {
            "yolo" => ApprovalDecision::Allow,
            "manual" => ApprovalDecision::Ask,
            "carrier" | "strict" => match tier {
                ApprovalTier::Read => ApprovalDecision::Allow,
                ApprovalTier::Write | ApprovalTier::Exec => ApprovalDecision::Ask,
            },
            _ => match tier {
                ApprovalTier::Read => ApprovalDecision::Allow,
                ApprovalTier::Write | ApprovalTier::Exec => ApprovalDecision::Ask,
            },
        }
    }
}
