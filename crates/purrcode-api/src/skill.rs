// SPDX-License-Identifier: GPL-3.0-or-later

//! Skill registry trait for the WebUI (v0.4 B5), mirroring
//! `plugin.rs` (B4). The channel crate only ever sees this trait,
//! so the crate chain stays one-way (`channel → api ← tools`):
//! the real implementation lives in `purrcode-cli` and routes
//! every mutation through the `manage_skill` tool — the HTTP
//! handlers never write a skill file directly.
//!
//! A skill is identified by name; where two scopes share a name,
//! the discovery precedence is project > global > embedded, and
//! the winning copy is the one `get_skill` returns. Mutations are
//! scope-addressed: `create` refuses a same-scope collision
//! (409), `update`/`delete` require the copy to exist in the
//! given scope (404 otherwise). Embedded (bundled) skills have no
//! file, so they are read-only — editing one means creating a
//! project/global copy with the same name, which then shadows it.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Where a skill comes from. Embedded skills ship in the binary;
/// global lives in `~/.purrcode/skills/`; project in
/// `<workspace>/.purrcode/skills/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Embedded,
    Global,
    Project,
}

/// The scope a mutation addresses. Embedded is not a scope: there
/// is no file to write, so embedded skills can only be shadowed by
/// creating a project/global copy of the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillScope {
    Project,
    Global,
}

/// One skill as the WebUI lists it: name, routing description,
/// source, and the file path when it has one (embedded skills
/// have none).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub path: Option<String>,
}

/// One skill with its full `SKILL.md` body, for the edit form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDetail {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    pub path: Option<String>,
    pub content: String,
}

/// Why a skill operation was refused. The WebUI maps these to
/// HTTP statuses: `NotFound` → 404, `AlreadyExists` → 409,
/// `InvalidInput` → 400, `Internal` → 500.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillError {
    NotFound(String),
    AlreadyExists(String),
    InvalidInput(String),
    Internal(String),
}

impl std::fmt::Display for SkillError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "no skill named '{name}'"),
            Self::AlreadyExists(name) => write!(f, "skill '{name}' already exists"),
            Self::InvalidInput(msg) => write!(f, "invalid skill input: {msg}"),
            Self::Internal(msg) => write!(f, "skill operation failed: {msg}"),
        }
    }
}

impl std::error::Error for SkillError {}

/// The live skill registry the WebUI manages (v0.4 B5).
/// Implemented in `purrcode-cli` on top of `manage_skill`; the
/// channel crate only ever sees this trait.
#[async_trait]
pub trait SkillRegistry: Send + Sync {
    /// Every skill the manager knows: the 8 embedded defaults
    /// plus global and project skills from disk, sorted by name.
    /// Precedence-resolved: one entry per name, the winning copy.
    async fn list_skills(&self) -> Vec<SkillInfo>;
    /// The winning copy of a skill with its full `SKILL.md` body.
    async fn get_skill(&self, name: &str) -> Result<SkillDetail, SkillError>;
    /// Create a skill via `manage_skill` (validates name and
    /// description, 100K content cap, writes the file). Refuses
    /// when a skill of the same name already exists in the scope.
    async fn create_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
    ) -> Result<SkillInfo, SkillError>;
    /// Replace a skill's content via `manage_skill` create (its
    /// create overwrites an existing file). The copy must exist
    /// in the scope first; embedded skills cannot be updated.
    async fn update_skill(
        &self,
        name: &str,
        description: &str,
        content: &str,
        scope: SkillScope,
    ) -> Result<SkillInfo, SkillError>;
    /// Delete a skill via `manage_skill`. The copy must exist in
    /// the scope; embedded skills cannot be deleted.
    async fn delete_skill(&self, name: &str, scope: SkillScope) -> Result<(), SkillError>;
}
