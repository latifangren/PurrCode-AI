// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::event::AgentEvent;
use purrcode_api::plugin::PluginHookDispatcher;
use purrcode_api::provider::{
    ChatMessage, CompletionRequest, MessageRole, ModelProvider, ToolCallDelta, ToolCallRequest,
};
use purrcode_api::tool::{
    ApprovalDecision, ApprovalTier, AuditSink, PolicyGate, ToolContext, ToolRegistry,
};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::steering::SteeringQueue;
use crate::EventBus;

#[async_trait]
pub trait ApprovalRequester: Send + Sync {
    /// `call_id` is the loop's tool-call id for this request: it lets a
    /// channel correlate the request with the `ToolCallApprovalRequested`
    /// event the loop publishes to the EventBus just before calling here.
    /// Requesters that render their own card (Telegram, TUI, CLI) may
    /// ignore it; the WebUI requester uses it as the pending key so the
    /// card the UI rendered from the bus event resolves to this wait.
    /// Plan-level consults with no tool call (Tuxeshadow in Carrier mode)
    /// pass a stable pseudo-id (`"shadow"`).
    async fn request_approval(
        &self,
        call_id: &str,
        tool: &str,
        description: &str,
        preview: Option<&str>,
    ) -> bool;
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
    temperature: Option<f32>,
    reasoning_effort: Option<String>,
    approval_requester: Option<Arc<dyn ApprovalRequester>>,
    policy_gate: Option<Arc<dyn PolicyGate>>,
    audit_sink: Option<Arc<dyn AuditSink>>,
    plugin_dispatcher: Option<Arc<dyn PluginHookDispatcher>>,
    is_yolo: bool,
    cancel_flag: Option<Arc<AtomicBool>>,
    steering_queue: Option<SteeringQueue>,
    /// Broadcast sink for turn events (R3). `None` = the loop runs
    /// listener-only, exactly like before; `Some` = every chunk, tool
    /// call, approval request and turn boundary is also published here
    /// (the WebUI subscribes through it; D5 `--json` will too).
    event_bus: Option<Arc<EventBus>>,
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
            temperature: Some(0.2),
            reasoning_effort: None,
            approval_requester: None,
            policy_gate: None,
            audit_sink: None,
            plugin_dispatcher: None,
            is_yolo: false,
            cancel_flag: None,
            steering_queue: None,
            event_bus: None,
        }
    }

    pub fn with_event_bus(mut self, bus: Arc<EventBus>) -> Self {
        self.event_bus = Some(bus);
        self
    }

    pub fn with_max_steps(mut self, max_steps: usize) -> Self {
        self.max_steps = max_steps;
        self
    }

    pub fn with_temperature(mut self, temperature: Option<f32>) -> Self {
        if temperature.is_some() {
            self.temperature = temperature;
        }
        self
    }

    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort;
        self
    }

    pub fn with_approval_requester(mut self, requester: Arc<dyn ApprovalRequester>) -> Self {
        self.approval_requester = Some(requester);
        self
    }

    pub fn with_policy_gate(mut self, gate: Arc<dyn PolicyGate>) -> Self {
        self.policy_gate = Some(gate);
        self
    }

    pub fn with_audit_sink(mut self, sink: Arc<dyn AuditSink>) -> Self {
        self.audit_sink = Some(sink);
        self
    }

    pub fn with_plugin_dispatcher(mut self, dispatcher: Arc<dyn PluginHookDispatcher>) -> Self {
        self.plugin_dispatcher = Some(dispatcher);
        self
    }

    pub fn with_yolo(mut self, yolo: bool) -> Self {
        self.is_yolo = yolo;
        self
    }

    pub fn with_cancel_flag(mut self, cancel_flag: Arc<AtomicBool>) -> Self {
        self.cancel_flag = Some(cancel_flag);
        self
    }

    pub fn with_steering_queue(mut self, queue: SteeringQueue) -> Self {
        self.steering_queue = Some(queue);
        self
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_flag
            .as_ref()
            .map(|f| f.load(Ordering::SeqCst))
            .unwrap_or(false)
    }

    fn record_audit(
        &self,
        ctx: &ToolContext,
        event: &str,
        tool: Option<&str>,
        decision: &str,
        detail: &str,
    ) {
        if let Some(ref sink) = self.audit_sink {
            sink.record_event(
                &ctx.session_id,
                &ctx.agent_role,
                event,
                tool,
                decision,
                detail,
            );
        }
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

        // One turn id for the whole turn: TurnStarted and TurnFinished
        // carry the same value so a consumer can pair them (the
        // dev-claude reference minted a second uuid at the end; pairing
        // by one id is strictly more useful and costs nothing).
        let turn_id = uuid::Uuid::new_v4().to_string();
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(AgentEvent::TurnStarted {
                session_id: ctx.session_id.clone(),
                turn_id: turn_id.clone(),
                timestamp: chrono::Utc::now(),
            });
        }

        let mut step = 0;
        let mut final_answer = String::new();

        while step < self.max_steps {
            if self.is_cancelled() {
                return Err(PurrError::Internal(
                    "Turn cancelled by user interrupt".to_string(),
                ));
            }

            if let Some(ref sq) = self.steering_queue {
                while let Some(steered) = sq.pop() {
                    messages.push(ChatMessage {
                        role: MessageRole::User,
                        content: steered,
                        tool_calls: None,
                        tool_call_id: None,
                    });
                }
            }

            step += 1;

            // One transparent retry per step when the provider stream
            // ends incomplete (truncated SSE: no finish_reason /
            // [DONE] — a flaky transport delivers a clean EOF, which
            // looks exactly like a model that "says nothing"). The
            // partial text/tool calls of a truncated attempt are
            // discarded: nothing was executed yet (tool calls run only
            // after the stream completes), and the conversation
            // history is untouched until the step succeeds. The
            // attempt's text is also HELD from the live stream until
            // the step completes cleanly (see the flush below): a
            // truncated attempt must never reach the one-shot output,
            // the TUI, or the WebUI SSE as a stale fragment that the
            // retry then duplicates. If the retry also truncates, the
            // turn fails honestly instead of returning partial output
            // as if it were complete.
            let mut step_text = String::new();
            let mut step_reasoning = String::new();
            let mut tool_calls_in_flight: HashMap<usize, (Option<String>, String, String)> =
                HashMap::new();

            for attempt in 0..=1u8 {
                step_text.clear();
                step_reasoning.clear();
                tool_calls_in_flight.clear();

                let request = CompletionRequest {
                    model: self.model.clone(),
                    messages: messages.clone(),
                    tools: Some(self.tool_registry.list()),
                    temperature: self.temperature,
                    max_tokens: None,
                    reasoning_effort: self.reasoning_effort.clone(),
                    stop: None,
                };

                let mut stream = self.provider.stream_completion(request).await?;

                let mut truncated = false;
                let mut truncation_detail = String::new();
                while let Some(chunk) = stream.recv().await {
                    if self.is_cancelled() {
                        return Err(PurrError::Internal(
                            "Turn cancelled by user interrupt".to_string(),
                        ));
                    }

                    if let Some(err) = chunk.stream_error {
                        // The provider stream died mid-response; the
                        // partial output of this attempt is
                        // untrustworthy — discard it and retry once.
                        truncated = true;
                        truncation_detail = err;
                        break;
                    }

                    if let Some(reasoning) = chunk.reasoning_delta {
                        // Held, not published: a truncated attempt is
                        // discarded whole, so its deltas must never
                        // reach the live stream (flushed below, on a
                        // clean step completion).
                        step_reasoning.push_str(&reasoning);
                    }

                    if !chunk.delta.is_empty() {
                        // Held, not published: see the reasoning
                        // buffer above.
                        step_text.push_str(&chunk.delta);
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

                if !truncated {
                    // The step completed cleanly: only now may its
                    // text reach the live stream. A truncated attempt
                    // never published anything, so a retry's text does
                    // not duplicate a stale fragment in the one-shot
                    // output, the TUI, or the WebUI SSE stream
                    // (re-checker P2-1, v0.3 polish fix pass). The
                    // trade-off is deliberate: the live stream now
                    // shows each step's text in one batch instead of
                    // word-by-word — the turn result, history, and
                    // audit are unchanged, and reasoning/text stay on
                    // their separate channels.
                    if !step_reasoning.is_empty() {
                        if let Some(ref bus) = self.event_bus {
                            let _ = bus.publish(AgentEvent::ReasoningChunk {
                                thinking: step_reasoning.clone(),
                            });
                        }
                        if let Some(l) = listener {
                            l.on_text_chunk(&step_reasoning, true);
                        }
                    }
                    if !step_text.is_empty() {
                        if let Some(ref bus) = self.event_bus {
                            let _ = bus.publish(AgentEvent::TextChunk {
                                text: step_text.clone(),
                            });
                        }
                        if let Some(l) = listener {
                            l.on_text_chunk(&step_text, false);
                        }
                    }
                    break;
                }
                if attempt == 1 {
                    return Err(PurrError::Provider {
                        provider: self.provider.provider_name().to_string(),
                        message: format!(
                            "provider stream ended incomplete twice in one step: {truncation_detail}"
                        ),
                    });
                }
                // Otherwise: retry the step with a fresh completion.
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
                final_answer = step_text;
                messages.push(ChatMessage {
                    role: MessageRole::Assistant,
                    content: final_answer.clone(),
                    tool_calls: None,
                    tool_call_id: None,
                });
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
                let args_val: serde_json::Value =
                    serde_json::from_str(&args_str).unwrap_or_else(|_| serde_json::json!({}));

                if let Some(ref bus) = self.event_bus {
                    let _ = bus.publish(AgentEvent::ToolCallStarted {
                        call_id: call_id.clone(),
                        tool: tool_name.clone(),
                        arguments: args_val.clone(),
                    });
                }

                if let Some(l) = listener {
                    l.on_tool_call_started(&tool_name, &args_str);
                }

                let (tool_output, is_success) = if let Some(tool) =
                    self.tool_registry.get(&tool_name)
                {
                    // 1. Dispatch `pre_tool_use` plugin hook (can veto execution)
                    let plugin_allowed = if let Some(ref pd) = self.plugin_dispatcher {
                        pd.dispatch_pre_tool_use(&tool_name, &args_val).await
                    } else {
                        true
                    };

                    if !plugin_allowed {
                        self.record_audit(
                            ctx,
                            "plugin_hook",
                            Some(&tool_name),
                            "vetoed_by_plugin",
                            &args_str,
                        );
                        (
                            format!(
                                "Action vetoed by pre_tool_use plugin hook for tool '{}'.",
                                tool_name
                            ),
                            false,
                        )
                    } else {
                        let def = tool.definition();
                        let outside_ws = is_args_outside_workspace(&args_val, &ctx.workspace_root);

                        // 2. Evaluate via wired PolicyGate (ApprovalGate) or default tier check
                        let gate_decision = if self.is_yolo && !outside_ws {
                            ApprovalDecision::Allow
                        } else if let Some(ref gate) = self.policy_gate {
                            gate.evaluate_tool(&tool_name, def.tier, outside_ws)
                        } else if matches!(def.tier, ApprovalTier::Write | ApprovalTier::Exec)
                            || outside_ws
                        {
                            ApprovalDecision::Ask
                        } else {
                            ApprovalDecision::Allow
                        };

                        let is_approved = match gate_decision {
                            ApprovalDecision::Allow => {
                                self.record_audit(
                                    ctx,
                                    "tool_call",
                                    Some(&tool_name),
                                    "allow",
                                    &args_str,
                                );
                                true
                            }
                            ApprovalDecision::Deny => {
                                self.record_audit(
                                    ctx,
                                    "tool_call",
                                    Some(&tool_name),
                                    "deny",
                                    &args_str,
                                );
                                false
                            }
                            ApprovalDecision::Ask => {
                                if let Some(ref req) = self.approval_requester {
                                    let mut dry_ctx = ctx.clone();
                                    dry_ctx.is_dry_run = true;
                                    let preview =
                                        match tool.execute(args_val.clone(), &dry_ctx).await {
                                            Ok(dry_res) => dry_res.diff_preview,
                                            Err(_) => None,
                                        };
                                    let desc = format!("Run tool '{}'", tool_name);
                                    // The requester is called with the
                                    // loop's call id so its card resolves
                                    // to exactly this event (R3).
                                    if let Some(ref bus) = self.event_bus {
                                        let _ =
                                            bus.publish(AgentEvent::ToolCallApprovalRequested {
                                                call_id: call_id.clone(),
                                                tool: tool_name.clone(),
                                                description: desc.clone(),
                                                diff_preview: preview.clone(),
                                            });
                                    }
                                    let user_ok = req
                                        .request_approval(
                                            &call_id,
                                            &tool_name,
                                            &desc,
                                            preview.as_deref(),
                                        )
                                        .await;
                                    self.record_audit(
                                        ctx,
                                        "tool_approval",
                                        Some(&tool_name),
                                        if user_ok {
                                            "approved_by_user"
                                        } else {
                                            "denied_by_user"
                                        },
                                        &args_str,
                                    );
                                    user_ok
                                } else {
                                    // Supervised fail-closed! If no approval requester is wired, deny destructive actions.
                                    self.record_audit(
                                        ctx,
                                        "tool_approval",
                                        Some(&tool_name),
                                        "denied_fail_closed",
                                        &args_str,
                                    );
                                    false
                                }
                            }
                        };

                        if !is_approved {
                            (
                                    format!(
                                        "Action denied by approval policy: execution of '{}' was rejected.",
                                        tool_name
                                    ),
                                    false,
                                )
                        } else {
                            let exec_res = match tool.execute(args_val, ctx).await {
                                Ok(res) => (res.output, true),
                                Err(e) => (
                                    format!("Error executing tool '{}': {}", tool_name, e),
                                    false,
                                ),
                            };
                            if let Some(ref pd) = self.plugin_dispatcher {
                                pd.dispatch_post_tool_use(
                                    &tool_name,
                                    &serde_json::json!({
                                        "output": exec_res.0,
                                        "success": exec_res.1
                                    }),
                                )
                                .await;
                            }
                            exec_res
                        }
                    }
                } else {
                    self.record_audit(
                        ctx,
                        "tool_call",
                        Some(&tool_name),
                        "tool_not_in_preset",
                        &args_str,
                    );
                    (
                        format!(
                            "Error: tool '{}' is not in preset for agent '{}'",
                            tool_name, ctx.agent_role
                        ),
                        false,
                    )
                };

                if let Some(ref bus) = self.event_bus {
                    let _ = bus.publish(AgentEvent::ToolCallFinished {
                        call_id: call_id.clone(),
                        tool: tool_name.clone(),
                        result: serde_json::json!({ "output": tool_output }),
                        success: is_success,
                    });
                }

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

        // ARCHITECTURE §2: when max_steps is reached, force a final text answer with tools disabled
        if step >= self.max_steps && final_answer.is_empty() {
            let final_req = CompletionRequest {
                model: self.model.clone(),
                messages: messages.clone(),
                tools: None,
                temperature: self.temperature,
                max_tokens: None,
                reasoning_effort: self.reasoning_effort.clone(),
                stop: None,
            };
            if let Ok(mut stream) = self.provider.stream_completion(final_req).await {
                // Same hold-until-clean rule as the retry loop above:
                // a truncated follow-up must not leak its partial
                // text into the live stream, nor stand as the final
                // answer — the fallback message below is the honest
                // outcome instead.
                let mut followup_text = String::new();
                let mut followup_truncated = false;
                while let Some(chunk) = stream.recv().await {
                    if chunk.stream_error.is_some() {
                        followup_truncated = true;
                        break;
                    }
                    if !chunk.delta.is_empty() {
                        followup_text.push_str(&chunk.delta);
                    }
                    if chunk.is_finished {
                        break;
                    }
                }
                if !followup_truncated && !followup_text.is_empty() {
                    if let Some(ref bus) = self.event_bus {
                        let _ = bus.publish(AgentEvent::TextChunk {
                            text: followup_text.clone(),
                        });
                    }
                    if let Some(l) = listener {
                        l.on_text_chunk(&followup_text, false);
                    }
                    final_answer.push_str(&followup_text);
                }
            }
            if final_answer.is_empty() {
                final_answer = format!(
                    "[Max steps ({}) reached; summarizing completed tool steps.]",
                    self.max_steps
                );
                // The fallback IS the turn's answer, so it must reach
                // the live stream like any other final text — the
                // one-shot output, the TUI, and the WebUI SSE render
                // only bus/listener events. Without this, a turn whose
                // follow-up stream truncated (or was empty) returned
                // the honest fallback while every live surface stayed
                // silent (re-checker P2-1e, round 2).
                if let Some(ref bus) = self.event_bus {
                    let _ = bus.publish(AgentEvent::TextChunk {
                        text: final_answer.clone(),
                    });
                }
                if let Some(l) = listener {
                    l.on_text_chunk(&final_answer, false);
                }
            }
        }

        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(AgentEvent::TurnFinished {
                session_id: ctx.session_id.clone(),
                turn_id,
                tokens_used: None,
            });
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
        self.run_turn_with_history(&mut messages, prompt, system_prompt, ctx, Some(&listener))
            .await
    }
}

fn normalize_lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            Component::Normal(c) => out.push(c),
        }
    }
    out
}

fn is_args_outside_workspace(args: &serde_json::Value, workspace_root: &Path) -> bool {
    let candidate_str = args
        .get("path")
        .and_then(|v| v.as_str())
        .or_else(|| args.get("workdir").and_then(|v| v.as_str()));

    let Some(raw) = candidate_str else {
        return false;
    };

    let abs_root = if workspace_root.is_absolute() {
        workspace_root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(workspace_root)
    };
    let canon_root =
        std::fs::canonicalize(&abs_root).unwrap_or_else(|_| normalize_lexical(&abs_root));
    let p = Path::new(raw);
    let raw_candidate = if p.is_absolute() {
        p.to_path_buf()
    } else {
        canon_root.join(p)
    };

    let normalized = normalize_lexical(&raw_candidate);
    if !normalized.starts_with(&canon_root) {
        return true;
    }

    // Resolve symlinks on existing target or parent directory to prevent symlink escape
    if normalized.exists() {
        if let Ok(canon_target) = std::fs::canonicalize(&normalized) {
            if !canon_target.starts_with(&canon_root) {
                return true;
            }
        }
    } else if let Some(parent) = normalized.parent() {
        if parent.exists() {
            if let Ok(canon_parent) = std::fs::canonicalize(parent) {
                if !canon_parent.starts_with(&canon_root) {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests;
