use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{
    ChatMessage, CompletionRequest, MessageRole, ModelProvider, ToolCallDelta, ToolCallRequest,
};
use purrcode_api::tool::ToolContext;
use purrcode_tools::ToolRegistry;
use std::collections::HashMap;
use std::sync::Arc;

pub struct AgentLoop {
    provider: Arc<dyn ModelProvider>,
    model: String,
    tool_registry: Arc<ToolRegistry>,
    max_steps: usize,
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
        }
    }

    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps;
        self
    }

    /// Runs the turn loop until the model gives a final textual answer or hits max_steps
    pub async fn run_turn<F>(
        &self,
        prompt: &str,
        system_prompt: Option<&str>,
        ctx: &ToolContext,
        mut on_text_chunk: F,
    ) -> Result<String>
    where
        F: FnMut(&str, bool), // (chunk_text, is_reasoning)
    {
        let mut messages: Vec<ChatMessage> = Vec::new();

        if let Some(sys) = system_prompt {
            messages.push(ChatMessage {
                role: MessageRole::System,
                content: sys.to_string(),
                tool_calls: None,
                tool_call_id: None,
            });
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
                    on_text_chunk(&reasoning, true);
                }

                if !chunk.delta.is_empty() {
                    step_text.push_str(&chunk.delta);
                    on_text_chunk(&chunk.delta, false);
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

                let tool_output = if let Some(tool) = self.tool_registry.get(&tool_name) {
                    let args_val: serde_json::Value = serde_json::from_str(&args_str)
                        .unwrap_or_else(|_| serde_json::json!({}));

                    match tool.execute(args_val, ctx).await {
                        Ok(res) => res.output,
                        Err(e) => format!("Error executing tool '{}': {}", tool_name, e),
                    }
                } else {
                    format!("Error: tool '{}' not found in registry", tool_name)
                };

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
}
