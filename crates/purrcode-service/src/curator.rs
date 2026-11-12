// SPDX-License-Identifier: GPL-3.0-or-later

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use purrcode_api::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillTelemetryRecord {
    pub skill_name: String,
    pub timestamp: DateTime<Utc>,
    pub success: bool,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillTelemetryStore {
    #[serde(default)]
    pub records: Vec<SkillTelemetryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillStatus {
    Active,
    Stale,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandidateSkill {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SkillAuditReport {
    pub skill_name: String,
    pub status: SkillStatus,
    pub last_used: Option<DateTime<Utc>>,
    pub total_calls: usize,
    pub action_taken: Option<String>,
}

pub struct CuratorEngine {
    skills_dir: PathBuf,
    telemetry_file: PathBuf,
    store: RwLock<SkillTelemetryStore>,
}

impl CuratorEngine {
    pub fn new(purrcode_dir: impl Into<PathBuf>) -> Self {
        let root = purrcode_dir.into();
        let skills_dir = root.join("skills");
        let telemetry_file = root.join("skills_telemetry.json");
        let store = Self::load_telemetry(&telemetry_file).unwrap_or_default();
        Self {
            skills_dir,
            telemetry_file,
            store: RwLock::new(store),
        }
    }

    fn load_telemetry(path: &Path) -> Result<SkillTelemetryStore> {
        if !path.exists() {
            return Ok(SkillTelemetryStore::default());
        }
        let data = std::fs::read_to_string(path)?;
        let store: SkillTelemetryStore = serde_json::from_str(&data)?;
        Ok(store)
    }

    async fn save_telemetry(&self) -> Result<()> {
        if let Some(parent) = self.telemetry_file.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        let store = self.store.read().await;
        let data = serde_json::to_string_pretty(&*store)?;
        tokio::fs::write(&self.telemetry_file, data).await?;
        Ok(())
    }

    pub async fn record_usage(
        &self,
        skill_name: &str,
        success: bool,
        duration_ms: u64,
    ) -> Result<()> {
        let record = SkillTelemetryRecord {
            skill_name: skill_name.to_string(),
            timestamp: Utc::now(),
            success,
            duration_ms,
        };
        {
            let mut store = self.store.write().await;
            store.records.push(record);
            // Limit telemetry records to last 10,000 entries
            if store.records.len() > 10_000 {
                let excess = store.records.len() - 10_000;
                store.records.drain(0..excess);
            }
        }
        self.save_telemetry().await?;
        Ok(())
    }

    /// Evaluates installed skills against telemetry.
    /// Inactivity > 14 days => flagged as Stale.
    /// Inactivity > 30 days => moved to `archive/` (unless pinned: true).
    pub async fn evaluate_skills(&self) -> Result<Vec<SkillAuditReport>> {
        let now = Utc::now();
        let archive_dir = self.skills_dir.join("archive");

        // Compute last used timestamp and call counts per skill
        let mut skill_stats: HashMap<String, (Option<DateTime<Utc>>, usize)> = HashMap::new();
        {
            let store = self.store.read().await;
            for rec in &store.records {
                let entry = skill_stats
                    .entry(rec.skill_name.clone())
                    .or_insert((None, 0));
                entry.1 += 1;
                if entry.0.is_none() || entry.0.unwrap() < rec.timestamp {
                    entry.0 = Some(rec.timestamp);
                }
            }
        }

        let mut reports = Vec::new();

        if !self.skills_dir.exists() {
            return Ok(reports);
        }

        let mut read_dir = tokio::fs::read_dir(&self.skills_dir).await?;

        while let Some(entry) = read_dir.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();

            if dir_name == "archive" || dir_name == "candidates" {
                continue;
            }

            let skill_file = path.join("SKILL.md");
            if !skill_file.exists() {
                continue;
            }

            // Check if skill is pinned in frontmatter
            let content = tokio::fs::read_to_string(&skill_file)
                .await
                .unwrap_or_default();
            let is_pinned = content.lines().any(|l| {
                let trimmed = l.trim();
                trimmed.starts_with("pinned:") && trimmed.ends_with("true")
            });

            let (last_used, calls) = skill_stats.get(&dir_name).cloned().unwrap_or((None, 0));

            let mut status = SkillStatus::Active;
            let mut action = None;

            if let Some(last) = last_used {
                let age = now - last;
                if age > ChronoDuration::days(30) {
                    if is_pinned {
                        status = SkillStatus::Stale;
                        action = Some("Exempt from archiving (pinned: true)".to_string());
                    } else {
                        // Move to archive
                        tokio::fs::create_dir_all(&archive_dir).await?;
                        let target = archive_dir.join(&dir_name);
                        if let Err(e) = tokio::fs::rename(&path, &target).await {
                            tracing::warn!("Failed to archive skill {}: {}", dir_name, e);
                        } else {
                            status = SkillStatus::Archived;
                            action = Some(format!("Archived to {:?}", target));
                        }
                    }
                } else if age > ChronoDuration::days(14) {
                    status = SkillStatus::Stale;
                    action = Some("Marked as stale (>14 days inactive)".to_string());
                }
            }

            reports.push(SkillAuditReport {
                skill_name: dir_name,
                status,
                last_used,
                total_calls: calls,
                action_taken: action,
            });
        }

        Ok(reports)
    }

    /// Proposes a new candidate skill based on automated workflow synthesis
    pub async fn propose_candidate_skill(
        &self,
        name: &str,
        description: &str,
        instructions: &str,
    ) -> Result<PathBuf> {
        let candidates_dir = self.skills_dir.join("candidates").join(name);
        tokio::fs::create_dir_all(&candidates_dir).await?;

        let skill_file = candidates_dir.join("SKILL.md");
        let content = format!(
            "---\nname: {}\ndescription: {}\npinned: false\n---\n\n# {}\n\n{}\n",
            name, description, name, instructions
        );

        tokio::fs::write(&skill_file, content).await?;

        Ok(skill_file)
    }

    /// Lists all candidate skills awaiting user approval
    pub async fn list_candidates(&self) -> Result<Vec<CandidateSkill>> {
        let candidates_dir = self.skills_dir.join("candidates");
        let mut list = Vec::new();
        if !candidates_dir.exists() {
            return Ok(list);
        }
        let mut rd = tokio::fs::read_dir(&candidates_dir).await?;
        while let Some(entry) = rd.next_entry().await? {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            let skill_file = path.join("SKILL.md");
            if skill_file.exists() {
                let content = tokio::fs::read_to_string(&skill_file)
                    .await
                    .unwrap_or_default();
                let desc = content
                    .lines()
                    .find(|l| l.trim().starts_with("description:"))
                    .map(|l| {
                        l.trim()
                            .trim_start_matches("description:")
                            .trim()
                            .to_string()
                    })
                    .unwrap_or_else(|| "Candidate skill".to_string());
                list.push(CandidateSkill {
                    name,
                    description: desc,
                    path: skill_file,
                });
            }
        }
        list.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(list)
    }

    /// Approves and promotes a candidate skill into active skills/
    pub async fn approve_candidate(&self, name: &str) -> Result<PathBuf> {
        let candidate_path = self.skills_dir.join("candidates").join(name);
        if !candidate_path.exists() {
            return Err(purrcode_api::error::PurrError::Tool {
                tool: "curator".to_string(),
                message: format!("Candidate skill '{}' not found", name),
            });
        }
        let target_path = self.skills_dir.join(name);
        if target_path.exists() {
            tokio::fs::remove_dir_all(&target_path).await?;
        }
        tokio::fs::rename(&candidate_path, &target_path).await?;
        Ok(target_path.join("SKILL.md"))
    }

    /// Rejects and deletes a candidate skill
    pub async fn reject_candidate(&self, name: &str) -> Result<()> {
        let candidate_path = self.skills_dir.join("candidates").join(name);
        if candidate_path.exists() {
            tokio::fs::remove_dir_all(&candidate_path).await?;
        }
        Ok(())
    }

    /// PRD §4.3 F20: Synthesizes a candidate skill from a complex task trajectory (5+ tool calls)
    pub async fn propose_from_trajectory(
        &self,
        name: &str,
        intent: &str,
        tool_calls: &[String],
    ) -> Result<Option<PathBuf>> {
        if tool_calls.len() < 5 {
            return Ok(None);
        }

        let instructions = format!(
            "### Execution Strategy\nThis skill automates the workflow for '{}' which originally required {} steps:\n\n{}",
            intent,
            tool_calls.len(),
            tool_calls
                .iter()
                .enumerate()
                .map(|(i, t)| format!("{}. Tool `{}`", i + 1, t))
                .collect::<Vec<_>>()
                .join("\n")
        );

        let path = self
            .propose_candidate_skill(name, intent, &instructions)
            .await?;
        Ok(Some(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_curator_telemetry_and_evaluate() {
        let temp_dir =
            std::env::temp_dir().join(format!("purrcode_curator_test_{}", uuid::Uuid::new_v4()));
        let skills_dir = temp_dir.join("skills");
        tokio::fs::create_dir_all(&skills_dir).await.unwrap();

        // Create an active skill
        let active_skill = skills_dir.join("active-tool");
        tokio::fs::create_dir_all(&active_skill).await.unwrap();
        tokio::fs::write(
            active_skill.join("SKILL.md"),
            "---\nname: active-tool\n---\n",
        )
        .await
        .unwrap();

        // Create an old skill to be archived
        let old_skill = skills_dir.join("old-tool");
        tokio::fs::create_dir_all(&old_skill).await.unwrap();
        tokio::fs::write(old_skill.join("SKILL.md"), "---\nname: old-tool\n---\n")
            .await
            .unwrap();

        // Create a pinned old skill
        let pinned_skill = skills_dir.join("pinned-tool");
        tokio::fs::create_dir_all(&pinned_skill).await.unwrap();
        tokio::fs::write(
            pinned_skill.join("SKILL.md"),
            "---\nname: pinned-tool\npinned: true\n---\n",
        )
        .await
        .unwrap();

        let curator = CuratorEngine::new(&temp_dir);

        // Record recent telemetry for active-tool
        curator
            .record_usage("active-tool", true, 120)
            .await
            .unwrap();

        // Inject old telemetry for old-tool (40 days ago)
        {
            let mut store = curator.store.write().await;
            store.records.push(SkillTelemetryRecord {
                skill_name: "old-tool".to_string(),
                timestamp: Utc::now() - ChronoDuration::days(40),
                success: true,
                duration_ms: 200,
            });
            store.records.push(SkillTelemetryRecord {
                skill_name: "pinned-tool".to_string(),
                timestamp: Utc::now() - ChronoDuration::days(40),
                success: true,
                duration_ms: 200,
            });
        }

        let reports = curator.evaluate_skills().await.unwrap();
        assert_eq!(reports.len(), 3);

        let old_rep = reports.iter().find(|r| r.skill_name == "old-tool").unwrap();
        assert_eq!(old_rep.status, SkillStatus::Archived);

        let pinned_rep = reports
            .iter()
            .find(|r| r.skill_name == "pinned-tool")
            .unwrap();
        assert_eq!(pinned_rep.status, SkillStatus::Stale);

        let active_rep = reports
            .iter()
            .find(|r| r.skill_name == "active-tool")
            .unwrap();
        assert_eq!(active_rep.status, SkillStatus::Active);

        // Verify old-tool was moved to archive/
        assert!(skills_dir
            .join("archive")
            .join("old-tool")
            .join("SKILL.md")
            .exists());
        assert!(!skills_dir.join("old-tool").exists());

        // Test proposing a candidate skill
        let cand_path = curator
            .propose_candidate_skill(
                "git-release",
                "Automates git tags and changelog",
                "Run cargo release",
            )
            .await
            .unwrap();
        assert!(cand_path.exists());

        // Test trajectory proposing (5+ tool calls)
        let trajectory = vec![
            "read".to_string(),
            "grep".to_string(),
            "find".to_string(),
            "edit".to_string(),
            "bash".to_string(),
        ];
        let traj_path = curator
            .propose_from_trajectory(
                "auto-refactor",
                "Refactor auth logic across multiple files",
                &trajectory,
            )
            .await
            .unwrap();
        assert!(traj_path.is_some());

        // Test list candidates
        let candidates = curator.list_candidates().await.unwrap();
        assert_eq!(candidates.len(), 2);
        assert!(candidates.iter().any(|c| c.name == "auto-refactor"));
        assert!(candidates.iter().any(|c| c.name == "git-release"));

        // Test approve candidate
        let approved_path = curator.approve_candidate("auto-refactor").await.unwrap();
        assert!(approved_path.exists());
        assert!(skills_dir.join("auto-refactor").join("SKILL.md").exists());
        assert!(!skills_dir.join("candidates").join("auto-refactor").exists());

        let _ = tokio::fs::remove_dir_all(&temp_dir).await;
    }
}
