pub mod meowstro;
pub mod task;

pub use meowstro::MeowstroOrchestrator;
pub use task::TaskTool;

use purrcode_api::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentRecord {
    pub id: String,
    pub parent_id: Option<String>,
    pub role: String,
    pub brief: String,
    pub result: Option<String>,
}

pub struct SubagentRegistry {
    records: HashMap<String, SubagentRecord>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    pub fn get_or_create(&mut self, id: Option<String>, role: &str, brief: &str) -> String {
        if let Some(existing_id) = id {
            if self.records.contains_key(&existing_id) {
                return existing_id;
            }
        }
        let new_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);
        self.records.insert(
            new_id.clone(),
            SubagentRecord {
                id: new_id.clone(),
                parent_id: None,
                role: role.to_string(),
                brief: brief.to_string(),
                result: None,
            },
        );
        new_id
    }

    pub fn record_result(&mut self, id: &str, result: &str) -> Result<()> {
        if let Some(record) = self.records.get_mut(id) {
            record.result = Some(result.to_string());
            Ok(())
        } else {
            Err(purrcode_api::PurrError::Subagent {
                id: id.to_string(),
                message: "Subagent record not found".to_string(),
            })
        }
    }
}

impl Default for SubagentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
