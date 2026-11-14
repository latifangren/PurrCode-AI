// SPDX-License-Identifier: GPL-3.0-or-later

use std::sync::Arc;

use purrcode_api::event::AgentEvent;
use purrcode_api::provider::{ChatMessage, MessageRole, ToolCallRequest};
use purrcode_api::tool::{ApprovalDecision, ApprovalTier, Tool, ToolContext};

use super::harness::{
    is_args_outside_workspace, is_parallel_read_eligible, FailureLoopTracker, SessionFileGuard,
};
use super::{AgentEventListener, AgentLoop};

struct ReadyRead {
    order: usize,
    call_id: String,
    tool_name: String,
    args_str: String,
    args_val: serde_json::Value,
    tool: Arc<dyn Tool>,
}

enum PendingOutcome {
    Ready(ReadyRead),
    Immediate {
        call_id: String,
        tool_name: String,
        args_str: String,
        output: String,
        success: bool,
    },
}

impl AgentLoop {
    /// Executes one step's requested tool calls: consecutive auto-allowed
    /// read-only calls run concurrently while mutating/interactive calls run
    /// sequentially in deterministic index order.
    pub(super) async fn execute_tool_batch(
        &self,
        executed_calls: &[ToolCallRequest],
        messages: &mut Vec<ChatMessage>,
        ctx: &ToolContext,
        listener: Option<&dyn AgentEventListener>,
        file_guard: &SessionFileGuard,
        failure_tracker: &mut FailureLoopTracker,
    ) {
        let mut idx = 0usize;
        while idx < executed_calls.len() {
            let mut ready_reads: Vec<ReadyRead> = Vec::new();
            let mut trailing_outcome: Option<PendingOutcome> = None;

            while idx < executed_calls.len() {
                let tool_call = &executed_calls[idx];
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

                let Some(tool) = self.tool_registry.get(&tool_name) else {
                    self.record_audit(
                        ctx,
                        "tool_call",
                        Some(&tool_name),
                        "tool_not_in_preset",
                        &args_str,
                    );
                    trailing_outcome = Some(PendingOutcome::Immediate {
                        call_id,
                        tool_name: tool_name.clone(),
                        args_str,
                        output: format!(
                            "Error: tool '{}' is not in preset for agent '{}'. Choose from the tools available in your active preset.",
                            tool_name, ctx.agent_role
                        ),
                        success: false,
                    });
                    idx += 1;
                    break;
                };

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
                    trailing_outcome = Some(PendingOutcome::Immediate {
                        call_id,
                        tool_name: tool_name.clone(),
                        args_str,
                        output: format!(
                            "Action vetoed by pre_tool_use plugin hook for tool '{}'. Adjust your approach instead of retrying verbatim.",
                            tool_name
                        ),
                        success: false,
                    });
                    idx += 1;
                    break;
                }

                // 2. Read-before-edit guard (only blocks mutating an existing unread file)
                if let Some(guard_err) =
                    file_guard.check_mutation_allowed(&ctx.workspace_root, &tool_name, &args_val)
                {
                    trailing_outcome = Some(PendingOutcome::Immediate {
                        call_id,
                        tool_name,
                        args_str,
                        output: guard_err,
                        success: false,
                    });
                    idx += 1;
                    break;
                }

                let def = tool.definition();
                let outside_ws = is_args_outside_workspace(&args_val, &ctx.workspace_root);

                // 3. Evaluate via wired PolicyGate (ApprovalGate) or default tier check
                let gate_decision = if self.is_yolo && !outside_ws {
                    ApprovalDecision::Allow
                } else if let Some(ref gate) = self.policy_gate {
                    gate.evaluate_tool(&tool_name, def.tier, outside_ws)
                } else if matches!(def.tier, ApprovalTier::Write | ApprovalTier::Exec) || outside_ws
                {
                    ApprovalDecision::Ask
                } else {
                    ApprovalDecision::Allow
                };

                if gate_decision == ApprovalDecision::Allow
                    && is_parallel_read_eligible(&tool_name, def.tier)
                {
                    self.record_audit(ctx, "tool_call", Some(&tool_name), "allow", &args_str);
                    ready_reads.push(ReadyRead {
                        order: idx,
                        call_id,
                        tool_name,
                        args_str,
                        args_val,
                        tool,
                    });
                    idx += 1;
                    continue;
                }

                // Non-parallel or interactive/mutating tool call: evaluate approval & break batch
                let is_approved = match gate_decision {
                    ApprovalDecision::Allow => {
                        self.record_audit(ctx, "tool_call", Some(&tool_name), "allow", &args_str);
                        true
                    }
                    ApprovalDecision::Deny => {
                        self.record_audit(ctx, "tool_call", Some(&tool_name), "deny", &args_str);
                        false
                    }
                    ApprovalDecision::Ask => {
                        if let Some(ref req) = self.approval_requester {
                            let mut dry_ctx = ctx.clone();
                            dry_ctx.is_dry_run = true;
                            let preview = match tool.execute(args_val.clone(), &dry_ctx).await {
                                Ok(dry_res) => dry_res.diff_preview,
                                Err(_) => None,
                            };
                            let desc = format!("Run tool '{}'", tool_name);
                            if let Some(ref bus) = self.event_bus {
                                let _ = bus.publish(AgentEvent::ToolCallApprovalRequested {
                                    call_id: call_id.clone(),
                                    tool: tool_name.clone(),
                                    description: desc.clone(),
                                    diff_preview: preview.clone(),
                                });
                            }
                            let user_ok = req
                                .request_approval(&call_id, &tool_name, &desc, preview.as_deref())
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
                    trailing_outcome = Some(PendingOutcome::Immediate {
                        call_id,
                        tool_name: tool_name.clone(),
                        args_str,
                        output: format!(
                            "Action denied by approval policy: execution of '{}' was rejected. Adjust your approach or ask the user instead of retrying the identical call.",
                            tool_name
                        ),
                        success: false,
                    });
                } else {
                    trailing_outcome = Some(PendingOutcome::Ready(ReadyRead {
                        order: idx,
                        call_id,
                        tool_name,
                        args_str,
                        args_val,
                        tool,
                    }));
                }
                idx += 1;
                break;
            }

            // Execute accumulated auto-allowed read batch (concurrently if >= 2)
            let mut completed: Vec<(
                usize,
                String,
                String,
                String,
                serde_json::Value,
                String,
                bool,
            )> = Vec::new();
            if ready_reads.len() >= 2 {
                let mut join_set = tokio::task::JoinSet::new();
                for item in ready_reads {
                    let ctx_cloned = ctx.clone();
                    join_set.spawn(async move {
                        let res = match item.tool.execute(item.args_val.clone(), &ctx_cloned).await
                        {
                            Ok(r) => (r.output, true),
                            Err(e) => (
                                format!("Error executing tool '{}': {}", item.tool_name, e),
                                false,
                            ),
                        };
                        (
                            item.order,
                            item.call_id,
                            item.tool_name,
                            item.args_str,
                            item.args_val,
                            res.0,
                            res.1,
                        )
                    });
                }
                while let Some(joined) = join_set.join_next().await {
                    if let Ok(entry) = joined {
                        completed.push(entry);
                    }
                }
                completed.sort_by_key(|entry| entry.0);
            } else if let Some(item) = ready_reads.into_iter().next() {
                let (out, ok) = match item.tool.execute(item.args_val.clone(), ctx).await {
                    Ok(r) => (r.output, true),
                    Err(e) => (
                        format!("Error executing tool '{}': {}", item.tool_name, e),
                        false,
                    ),
                };
                completed.push((
                    item.order,
                    item.call_id,
                    item.tool_name,
                    item.args_str,
                    item.args_val,
                    out,
                    ok,
                ));
            }

            // Finalize parallel read results in deterministic order
            for (_order, call_id, tool_name, args_str, args_val, mut tool_output, is_success) in
                completed
            {
                if let Some(ref pd) = self.plugin_dispatcher {
                    pd.dispatch_post_tool_use(
                        &tool_name,
                        &serde_json::json!({
                            "output": tool_output,
                            "success": is_success
                        }),
                    )
                    .await;
                }
                if is_success && tool_name == "read" {
                    if let Some(p) = args_val.get("path").and_then(|v| v.as_str()) {
                        file_guard.record_read(&ctx.workspace_root, p);
                    }
                }
                self.finish_tool_call(
                    &call_id,
                    &tool_name,
                    &args_str,
                    &mut tool_output,
                    is_success,
                    messages,
                    listener,
                    failure_tracker,
                );
            }

            // Execute trailing sequential/mutating/immediate outcome
            if let Some(outcome) = trailing_outcome {
                let (call_id, tool_name, args_str, mut tool_output, is_success) = match outcome {
                    PendingOutcome::Immediate {
                        call_id,
                        tool_name,
                        args_str,
                        output,
                        success,
                    } => (call_id, tool_name, args_str, output, success),
                    PendingOutcome::Ready(item) => {
                        let (out, ok) = match item.tool.execute(item.args_val.clone(), ctx).await {
                            Ok(r) => (r.output, true),
                            Err(e) => (
                                format!("Error executing tool '{}': {}", item.tool_name, e),
                                false,
                            ),
                        };
                        if let Some(ref pd) = self.plugin_dispatcher {
                            pd.dispatch_post_tool_use(
                                &item.tool_name,
                                &serde_json::json!({
                                    "output": out,
                                    "success": ok
                                }),
                            )
                            .await;
                        }
                        if ok && matches!(item.tool_name.as_str(), "read" | "write" | "edit") {
                            if let Some(p) = item.args_val.get("path").and_then(|v| v.as_str()) {
                                if item.tool_name == "read" {
                                    file_guard.record_read(&ctx.workspace_root, p);
                                } else {
                                    file_guard.record_write(&ctx.workspace_root, p);
                                }
                            }
                        }
                        (item.call_id, item.tool_name, item.args_str, out, ok)
                    }
                };

                self.finish_tool_call(
                    &call_id,
                    &tool_name,
                    &args_str,
                    &mut tool_output,
                    is_success,
                    messages,
                    listener,
                    failure_tracker,
                );
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn finish_tool_call(
        &self,
        call_id: &str,
        tool_name: &str,
        args_str: &str,
        tool_output: &mut String,
        is_success: bool,
        messages: &mut Vec<ChatMessage>,
        listener: Option<&dyn AgentEventListener>,
        failure_tracker: &mut FailureLoopTracker,
    ) {
        if let Some(reminder) = failure_tracker.record(tool_name, args_str, is_success) {
            tool_output.push_str(&reminder);
        }
        if let Some(ref bus) = self.event_bus {
            let _ = bus.publish(AgentEvent::ToolCallFinished {
                call_id: call_id.to_string(),
                tool: tool_name.to_string(),
                result: serde_json::json!({ "output": tool_output }),
                success: is_success,
            });
        }
        if let Some(l) = listener {
            l.on_tool_call_finished(tool_name, tool_output, is_success);
        }
        messages.push(ChatMessage {
            role: MessageRole::Tool,
            content: std::mem::take(tool_output),
            tool_calls: None,
            tool_call_id: Some(call_id.to_string()),
        });
    }
}
