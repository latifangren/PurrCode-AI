// SPDX-License-Identifier: GPL-3.0-or-later

pub mod meowstro;
pub mod shadow;
pub mod task;

pub use meowstro::MeowstroOrchestrator;
pub use shadow::{parse_shadow_verdict, should_trigger_shadow, ShadowAuditor, ShadowVerdict};
pub use task::{format_validated_result_markdown, resolve_role_model_spec, TaskTool};

use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::ChatMessage;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentTokenUsage {
    pub input: u64,
    pub output: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentRecord {
    pub id: String,
    pub parent_id: String,
    pub role: String,
    pub status: String,
    pub created_at: String,
    pub updated_at: String,
    pub tokens_used: SubagentTokenUsage,
    pub summary: Option<String>,
    #[serde(default)]
    pub brief: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WorkgraphNode {
    pub id: String,
    pub role: String,
    pub status: String,
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Workgraph {
    pub nodes: Vec<WorkgraphNode>,
}

pub struct SubagentRegistry {
    records: HashMap<String, SubagentRecord>,
    histories: HashMap<String, Vec<ChatMessage>>,
    session_dir: Option<PathBuf>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
            histories: HashMap::new(),
            session_dir: None,
        }
    }

    pub fn with_session_dir(session_dir: impl Into<PathBuf>) -> Self {
        let dir = session_dir.into();
        let mut reg = Self {
            records: HashMap::new(),
            histories: HashMap::new(),
            session_dir: Some(dir.clone()),
        };
        reg.load_from_disk(&dir);
        reg
    }

    pub fn load_from_disk(&mut self, session_dir: &Path) {
        let sub_dir = session_dir.join("subagents");
        if let Ok(entries) = std::fs::read_dir(&sub_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(rec) = serde_json::from_str::<SubagentRecord>(&content) {
                            let id = rec.id.clone();
                            self.records.insert(id.clone(), rec);
                            let hist_file = sub_dir.join(&id).join("history.json");
                            if let Ok(h_str) = std::fs::read_to_string(&hist_file) {
                                if let Ok(msgs) = serde_json::from_str::<Vec<ChatMessage>>(&h_str) {
                                    self.histories.insert(id, msgs);
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn get_or_create(&mut self, id: Option<String>, role: &str, brief: &str) -> Result<String> {
        let now = chrono::Utc::now().to_rfc3339();
        if let Some(existing_id) = id {
            if let Some(rec) = self.records.get_mut(&existing_id) {
                rec.status = "running".to_string();
                rec.updated_at = now;
                rec.brief = brief.to_string();
                self.persist_record(&existing_id)?;
                return Ok(existing_id);
            }
            if let Some(sdir) = self.session_dir.clone() {
                let rec_path = sdir.join("subagents").join(format!("{}.json", existing_id));
                if rec_path.exists() {
                    self.load_from_disk(&sdir);
                    if let Some(rec) = self.records.get_mut(&existing_id) {
                        rec.status = "running".to_string();
                        rec.updated_at = now;
                        rec.brief = brief.to_string();
                        self.persist_record(&existing_id)?;
                        return Ok(existing_id);
                    }
                }
            }
        }

        let new_id = format!("purr_{}", &uuid::Uuid::new_v4().to_string()[..6]);
        let record = SubagentRecord {
            id: new_id.clone(),
            parent_id: "meowstro_root".to_string(),
            role: role.to_string(),
            status: "running".to_string(),
            created_at: now.clone(),
            updated_at: now,
            tokens_used: SubagentTokenUsage {
                input: 0,
                output: 0,
            },
            summary: None,
            brief: brief.to_string(),
        };
        self.records.insert(new_id.clone(), record);
        self.persist_record(&new_id)?;
        self.persist_workgraph()?;
        Ok(new_id)
    }

    pub fn take_history(&mut self, id: &str) -> Vec<ChatMessage> {
        self.histories.remove(id).unwrap_or_default()
    }

    pub fn save_history(&mut self, id: &str, messages: Vec<ChatMessage>) -> Result<()> {
        if let Some(ref sdir) = self.session_dir {
            let sub_dir = sdir.join("subagents").join(id);
            std::fs::create_dir_all(&sub_dir).map_err(PurrError::Io)?;
            let serialized = serde_json::to_string_pretty(&messages)?;
            std::fs::write(sub_dir.join("history.json"), serialized.as_bytes())
                .map_err(PurrError::Io)?;
        }
        self.histories.insert(id.to_string(), messages);
        Ok(())
    }

    pub fn record_result(&mut self, id: &str, result: &str) -> Result<()> {
        if let Some(record) = self.records.get_mut(id) {
            record.status = "done".to_string();
            record.updated_at = chrono::Utc::now().to_rfc3339();
            record.summary = Some(result.to_string());
            self.persist_record(id)?;
            self.persist_workgraph()?;
            Ok(())
        } else {
            Err(PurrError::Subagent {
                id: id.to_string(),
                message: "Subagent record not found".to_string(),
            })
        }
    }

    pub fn record_failure(&mut self, id: &str, status: &str, reason: &str) -> Result<()> {
        if let Some(record) = self.records.get_mut(id) {
            record.status = status.to_string();
            record.updated_at = chrono::Utc::now().to_rfc3339();
            record.summary = Some(reason.to_string());
            self.persist_record(id)?;
            self.persist_workgraph()?;
            Ok(())
        } else {
            Err(PurrError::Subagent {
                id: id.to_string(),
                message: "Subagent record not found".to_string(),
            })
        }
    }

    pub fn get_record(&self, id: &str) -> Option<&SubagentRecord> {
        self.records.get(id)
    }

    pub fn list_records(&self) -> Vec<&SubagentRecord> {
        self.records.values().collect()
    }

    fn persist_record(&self, id: &str) -> Result<()> {
        if let (Some(ref sdir), Some(rec)) = (&self.session_dir, self.records.get(id)) {
            let sub_dir = sdir.join("subagents");
            std::fs::create_dir_all(&sub_dir).map_err(PurrError::Io)?;
            let json_path = sub_dir.join(format!("{}.json", id));
            let json_str = serde_json::to_string_pretty(rec)?;
            std::fs::write(json_path, json_str.as_bytes()).map_err(PurrError::Io)?;
        }
        Ok(())
    }

    fn persist_workgraph(&self) -> Result<()> {
        if let Some(ref sdir) = self.session_dir {
            std::fs::create_dir_all(sdir).map_err(PurrError::Io)?;
            let mut nodes: Vec<WorkgraphNode> = self
                .records
                .values()
                .map(|r| WorkgraphNode {
                    id: r.id.clone(),
                    role: r.role.clone(),
                    status: r.status.clone(),
                    depends_on: Vec::new(),
                })
                .collect();
            nodes.sort_by(|a, b| a.id.cmp(&b.id));
            let wg = Workgraph { nodes };
            let json_str = serde_json::to_string_pretty(&wg)?;
            std::fs::write(sdir.join("workgraph.json"), json_str.as_bytes())
                .map_err(PurrError::Io)?;
        }
        Ok(())
    }
}

impl Default for SubagentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
