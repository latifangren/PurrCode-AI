pub mod channel;
pub mod error;
pub mod event;
pub mod memory;
pub mod plugin;
pub mod provider;
pub mod tool;

pub use channel::{Channel, ChannelCapabilities};
pub use error::{PurrError, Result};
pub use event::{AgentEvent, SubagentStatus};
pub use memory::{MemoryBackend, MemoryFact};
pub use plugin::{Plugin, PluginCapabilities, PluginManifest, PluginTransport};
pub use provider::{ChatMessage, CompletionRequest, MessageRole, ModelProvider, StreamChunk, TokenUsage, ToolCallDelta, ToolCallRequest};
pub use tool::{ApprovalTier, Tool, ToolContext, ToolDefinition, ToolResult};
