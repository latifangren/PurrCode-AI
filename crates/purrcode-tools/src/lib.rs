// SPDX-License-Identifier: GPL-3.0-or-later

pub mod coordination;
pub mod execution;
pub mod fs;
pub mod mcp;
pub mod search;
pub mod skill;
pub mod web;

pub use coordination::{AskOption, AskRequester, AskTool, TodoTool};
pub use execution::BashTool;
pub use fs::{EditTool, ReadTool, WriteTool};
pub use purrcode_api::tool::ToolRegistry;
pub use search::{FindTool, GlobTool, GrepTool};
pub use skill::{ManageSkillTool, ReadSkillTool, SkillManager};
pub use web::{WebfetchTool, WebsearchTool};

use std::sync::Arc;
use tokio::sync::Mutex;

pub trait ToolRegistryExt {
    fn with_defaults() -> Self;
    fn with_defaults_and_ask(ask_requester: Option<Arc<dyn AskRequester>>) -> Self;
    fn with_defaults_and_options(
        ask_requester: Option<Arc<dyn AskRequester>>,
        workspace_root: Option<&std::path::Path>,
    ) -> Self;
}

impl ToolRegistryExt for ToolRegistry {
    fn with_defaults() -> Self {
        Self::with_defaults_and_ask(None)
    }

    fn with_defaults_and_ask(ask_requester: Option<Arc<dyn AskRequester>>) -> Self {
        Self::with_defaults_and_options(ask_requester, None)
    }

    fn with_defaults_and_options(
        ask_requester: Option<Arc<dyn AskRequester>>,
        workspace_root: Option<&std::path::Path>,
    ) -> Self {
        let mut registry = Self::new();
        let _ = registry.register(Arc::new(ReadTool));
        let _ = registry.register(Arc::new(WriteTool));
        let _ = registry.register(Arc::new(EditTool));
        let _ = registry.register(Arc::new(BashTool));
        let _ = registry.register(Arc::new(GrepTool));
        let _ = registry.register(Arc::new(GlobTool));
        let _ = registry.register(Arc::new(FindTool));
        let _ = registry.register(Arc::new(TodoTool::new()));
        let ask_tool = match ask_requester {
            Some(req) => AskTool::with_requester(req),
            None => AskTool::new(),
        };
        let _ = registry.register(Arc::new(ask_tool));
        let _ = registry.register(Arc::new(WebfetchTool::new()));
        let _ = registry.register(Arc::new(WebsearchTool::new()));

        let mut skill_mgr = SkillManager::new();
        let default_root =
            std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let ws_root = workspace_root.unwrap_or(&default_root);
        skill_mgr.discover_all(ws_root);
        let shared_skill_mgr = Arc::new(Mutex::new(skill_mgr));
        let _ = registry.register(Arc::new(ReadSkillTool::new(Arc::clone(&shared_skill_mgr))));
        let _ = registry.register(Arc::new(ManageSkillTool::new(shared_skill_mgr)));

        registry
    }
}
