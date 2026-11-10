use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{
    ChatMessage, CompletionRequest, MessageRole, ModelProvider, ToolCallDelta, ToolCallRequest,
};
use purrcode_api::tool::ToolContext;
use purrcode_tools::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use purrcode_api::tool::ApprovalTier;

#[async_trait]
pub trait ApprovalRequester: Send + Sync {
    async fn request_approval(&self, tool: &str, description: &str, preview: Option<&str>) -> bool;
}

pub trait AgentEventListener: Send + Sync {
    fn on_text_chunk(&self, _chunk: &str, _is_reasoning: bool) {}
    fn on_tool_call_started(&self, _name: &str, _args: &str) {}
    fn on_tool_call_finished(&self, _name: &str, _output: &str, _success: bool) {}
}

pub struct AgentLoop {
    provider: Arc<dyn ModelProvider>,
    model: String,
    tool_registry: Arc<ToolRegistry>,
    max_steps: usize,
    approval_requester: Option<Arc<dyn ApprovalRequester>>,
    is_yolo: bool,
}

impl AgentLoop {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        tool_registry: Arc<ToolRegistry>,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            tool_registry,
            max_steps: 25,
            approval_requester: None,
            is_yolo: false,
        }
    }

    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps;
        self
    }

    pub fn with_approval_requester(mut self, requester: Arc<dyn ApprovalRequester>) -> Self {
        self.approval_requester = Some(requester);
        self
    }

    pub fn with_yolo(mut self, yolo: bool) -> Self {
        self.is_yolo = yolo;
        self
    }

    /// Runs a turn with persistent multi-turn message history and optional event listener
    pub async fn run_turn_with_history(
        &self,
        messages: &mut Vec<ChatMessage>,
        prompt: &str,
        system_prompt: Option<&str>,
        ctx: &ToolContext,
        listener: Option<&dyn AgentEventListener>,
    ) -> Result<String> {
        if messages.is_empty() {
            if let Some(sys) = system_prompt {
                messages.push(ChatMessage {
                    role: MessageRole::System,
                    content: sys.to_string(),
                    tool_calls: None,
                    tool_call_id: None,
                });
            }
        }

        messages.push(ChatMessage {
            role: MessageRole::User,
            content: prompt.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let mut step = 0;
        let mut final_answer = String::new();

        while step < self.max_steps {
            step += 1;

            let request = CompletionRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: Some(self.tool_registry.list()),
                temperature: Some(0.2),
                max_tokens: None,
                reasoning_effort: None,
                stop: None,
            };

            let mut stream = self.provider.stream_completion(request).await?;

            let mut step_text = String::new();
            let mut tool_calls_in_flight: HashMap<usize, (Option<String>, String, String)> = HashMap::new();

            while let Some(chunk) = stream.recv().await {
                if let Some(reasoning) = chunk.reasoning_delta {
                    if let Some(l) = listener {
                        l.on_text_chunk(&reasoning, true);
                    }
                }

                if !chunk.delta.is_empty() {
                    step_text.push_str(&chunk.delta);
                    if let Some(l) = listener {
                        l.on_text_chunk(&chunk.delta, false);
                    }
                }

                if let Some(ToolCallDelta {
                    index,
                    id,
                    name,
                    arguments_delta,
                }) = chunk.tool_call_delta
                {
                    let entry = tool_calls_in_flight
                        .entry(index)
                        .or_insert_with(|| (None, String::new(), String::new()));
                    if let Some(new_id) = id {
                        entry.0 = Some(new_id);
                    }
                    if let Some(new_name) = name {
                        entry.1.push_str(&new_name);
                    }
                    if let Some(args_delta) = arguments_delta {
                        entry.2.push_str(&args_delta);
                    }
                }

                if chunk.is_finished {
                    break;
                }
            }

            // Convert tool_calls map to sorted Vec
            let mut executed_calls: Vec<ToolCallRequest> = Vec::new();
            let mut indices: Vec<usize> = tool_calls_in_flight.keys().cloned().collect();
            indices.sort_unstable();

            for idx in indices {
                if let Some((id_opt, name, args)) = tool_calls_in_flight.remove(&idx) {
                    let call_id = id_opt.unwrap_or_else(|| format!("call_{}_{}", step, idx));
                    executed_calls.push(ToolCallRequest {
                        id: call_id,
                        name,
                        arguments: args,
                    });
                }
            }

            if executed_calls.is_empty() {
                // Model provided final answer without calling any further tools
                final_answer = step_text;
                break;
            }

            // Append assistant response with tool calls to conversation history
            messages.push(ChatMessage {
                role: MessageRole::Assistant,
                content: step_text,
                tool_calls: Some(executed_calls.clone()),
                tool_call_id: None,
            });

            // Execute each requested tool call
            for tool_call in executed_calls {
                let call_id = tool_call.id.clone();
                let tool_name = tool_call.name.clone();
                let args_str = tool_call.arguments.clone();

                if let Some(l) = listener {
                    l.on_tool_call_started(&tool_name, &args_str);
                }

                let (tool_output, is_success) = if let Some(tool) = self.tool_registry.get(&tool_name) {
                    let args_val: serde_json::Value = serde_json::from_str(&args_str)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    let def = tool.definition();
                    let is_destructive = matches!(def.tier, ApprovalTier::Write | ApprovalTier::Exec);

                    let is_approved = if !is_destructive || self.is_yolo {
                        true
                    } else if let Some(ref req) = self.approval_requester {
                        req.request_approval(&tool_name, &format!("Run tool '{}'", tool_name), None).await
                    } else {
                        // Supervised fail-closed! If no approval requester is wired, deny destructive actions.
                        false
                    };

                    if !is_approved {
                        (format!("Action denied by approval policy: execution of '{}' requires explicit confirmation.", tool_name), false)
                    } else {
                        match tool.execute(args_val, ctx).await {
                            Ok(res) => (res.output, true),
                            Err(e) => (format!("Error executing tool '{}': {}", tool_name, e), false),
                        }
                    }
                } else {
                    (format!("Error: tool '{}' not found in registry", tool_name), false)
                };

                if let Some(l) = listener {
                    l.on_tool_call_finished(&tool_name, &tool_output, is_success);
                }

                messages.push(ChatMessage {
                    role: MessageRole::Tool,
                    content: tool_output,
                    tool_calls: None,
                    tool_call_id: Some(call_id),
                });
            }
        }

        if step >= self.max_steps && final_answer.is_empty() {
            return Err(PurrError::Internal(format!(
                "Agent reached max steps ({}) without completing turn.",
                self.max_steps
            )));
        }

        Ok(final_answer)
    }

    /// Runs a single turn without persisting external messages
    pub async fn run_turn<F>(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        ctx: &ToolContext,
        on_text_chunk: F,
    ) -> Result<String>
    where
        F: FnMut(&str, bool) + Send,
    {
        struct FnListener<T>(tokio::sync::Mutex<T>);
        impl<T: FnMut(&str, bool) + Send> AgentEventListener for FnListener<T> {
            fn on_text_chunk(&self, chunk: &str, is_reasoning: bool) {
                if let Ok(mut lock) = self.0.try_lock() {
                    (*lock)(chunk, is_reasoning);
                }
            }
        }
        let listener = FnListener(tokio::sync::Mutex::new(on_text_chunk));
        let mut messages = Vec::new();
        self.run_turn_with_history(&mut messages, prompt, system_prompt, ctx, Some(&listener)).await
    }
}
