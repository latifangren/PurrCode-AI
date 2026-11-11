// SPDX-License-Identifier: GPL-3.0-or-later

use async_trait::async_trait;
use futures_util::StreamExt;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{
    CompletionRequest, MessageRole, ModelProvider, StreamChunk, TokenUsage,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

pub struct OpenAiCompatibleProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    /// The User-Agent actually sent (custom value from config, or the
    /// PurrCode default). Stored for observability and unit tests.
    pub user_agent: String,
    client: reqwest::Client,
    healthy: AtomicBool,
}

/// Honest default User-Agent identifying this product. Strict WAFs reject
/// requests without a recognizable User-Agent (observed: the user's gateway
/// returns 403/error 1010 for a request with no UA header at all); this
/// default passes there. A browser UA is only needed if a gateway rejects
/// this default specifically (set `user_agent` in the provider config).
pub const DEFAULT_USER_AGENT: &str =
    "PurrCode-AI/0.1 (purrcode-ai; https://github.com/latifangren/PurrCode-AI)";

impl OpenAiCompatibleProvider {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: Option<String>,
        headers: HashMap<String, String>,
        user_agent: Option<String>,
    ) -> Self {
        let mut url = base_url.into();
        if url.ends_with('/') {
            url.pop();
        }
        let valid_url = !url.trim().is_empty() && reqwest::Url::parse(&url).is_ok();
        let configured = user_agent.unwrap_or_else(|| DEFAULT_USER_AGENT.to_string());
        // An invalid header value (e.g. control characters) would make the
        // builder fail, and the fallback client would then send NO
        // User-Agent at all — the exact headerless request strict WAFs
        // reject (observed: 403/error 1010). Validate first and fall back
        // to the honest default, so the stored value is always the UA
        // actually sent.
        let user_agent = if reqwest::header::HeaderValue::from_str(&configured).is_ok() {
            configured
        } else {
            DEFAULT_USER_AGENT.to_string()
        };
        let client = reqwest::Client::builder()
            .user_agent(user_agent.clone())
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        Self {
            name: name.into(),
            base_url: url,
            api_key,
            headers,
            user_agent,
            client,
            healthy: AtomicBool::new(valid_url),
        }
    }

    fn endpoint_url(&self) -> String {
        if self.base_url.ends_with("/v1") {
            format!("{}/chat/completions", self.base_url)
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        }
    }
}

use purrcode_api::provider::ToolCallDelta;

#[derive(Serialize)]
struct ChatCompletionRequestBody<'a> {
    model: &'a str,
    messages: Vec<ApiMessage<'a>>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ApiTool<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'a str>,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct ApiTool<'a> {
    r#type: &'static str,
    function: ApiFunction<'a>,
}

#[derive(Serialize)]
struct ApiFunction<'a> {
    name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
struct ApiMessage<'a> {
    role: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<ApiToolCall<'a>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<&'a str>,
}

#[derive(Serialize)]
struct ApiToolCall<'a> {
    id: &'a str,
    r#type: &'static str,
    function: ApiToolCallFunction<'a>,
}

#[derive(Serialize)]
struct ApiToolCallFunction<'a> {
    name: &'a str,
    arguments: &'a str,
}

#[derive(Deserialize)]
struct SseChunk {
    #[serde(default)]
    choices: Vec<SseChoice>,
    usage: Option<SseUsage>,
}

#[derive(Deserialize)]
struct SseChoice {
    delta: Option<SseDelta>,
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct SseDelta {
    content: Option<String>,
    // Reasoning fields across OpenAI / DeepSeek / Gemini / Qwen endpoints
    reasoning_content: Option<String>,
    reasoning: Option<String>,
    thought: Option<String>,
    tool_calls: Option<Vec<SseToolCallDelta>>,
}

#[derive(Deserialize)]
struct SseToolCallDelta {
    index: usize,
    id: Option<String>,
    function: Option<SseFunctionDelta>,
}

#[derive(Deserialize)]
struct SseFunctionDelta {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct SseUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
}

#[async_trait]
impl ModelProvider for OpenAiCompatibleProvider {
    fn provider_name(&self) -> &str {
        &self.name
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::SeqCst)
    }

    async fn stream_completion(
        &self,
        request: CompletionRequest,
    ) -> Result<tokio::sync::mpsc::Receiver<StreamChunk>> {
        let (tx, rx) = tokio::sync::mpsc::channel(64);

        let messages: Vec<ApiMessage> = request
            .messages
            .iter()
            .map(|m| ApiMessage {
                role: match m.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                },
                content: if m.content.is_empty() {
                    None
                } else {
                    Some(&m.content)
                },
                tool_calls: m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter()
                        .map(|tc| ApiToolCall {
                            id: &tc.id,
                            r#type: "function",
                            function: ApiToolCallFunction {
                                name: &tc.name,
                                arguments: &tc.arguments,
                            },
                        })
                        .collect()
                }),
                tool_call_id: m.tool_call_id.as_deref(),
            })
            .collect();

        let tools: Option<Vec<ApiTool>> = request.tools.as_ref().map(|ts| {
            ts.iter()
                .map(|t| ApiTool {
                    r#type: "function",
                    function: ApiFunction {
                        name: &t.name,
                        description: &t.description,
                        parameters: &t.parameters,
                    },
                })
                .collect()
        });

        let body = ChatCompletionRequestBody {
            model: &request.model,
            messages,
            stream: true,
            tools,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
            reasoning_effort: request.reasoning_effort.as_deref(),
            stream_options: StreamOptions {
                include_usage: true,
            },
        };

        let url = self.endpoint_url();
        let mut req_builder = self.client.post(&url).json(&body);

        if let Some(ref key) = self.api_key {
            req_builder = req_builder.header("Authorization", format!("Bearer {}", key));
        }

        for (k, v) in &self.headers {
            req_builder = req_builder.header(k, v);
        }

        let response = req_builder.send().await.map_err(|e| {
            self.healthy.store(false, Ordering::SeqCst);
            PurrError::Provider {
                provider: self.name.clone(),
                message: format!("HTTP request to {} failed: {}", url, e),
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            if status.is_server_error() {
                self.healthy.store(false, Ordering::SeqCst);
            }
            let err_text = response.text().await.unwrap_or_default();
            let lower = err_text.to_ascii_lowercase();
            if lower.contains("content_policy_violation")
                || lower.contains("safety_violation")
                || lower.contains("policy_refusal")
                || lower.contains("violates our usage policy")
            {
                return Err(PurrError::ProviderRefusal {
                    provider: self.name.clone(),
                    reason: format!("HTTP {}: {}", status, err_text),
                });
            }
            return Err(PurrError::Provider {
                provider: self.name.clone(),
                message: format!("HTTP error {}: {}", status, err_text),
            });
        }

        self.healthy.store(true, Ordering::SeqCst);

        tokio::spawn(async move {
            let mut byte_stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk_res) = byte_stream.next().await {
                let chunk = match chunk_res {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx
                            .send(StreamChunk {
                                delta: format!("\n[Stream read error: {}]", e),
                                reasoning_delta: None,
                                tool_call_delta: None,
                                is_finished: true,
                                usage: None,
                            })
                            .await;
                        break;
                    }
                };

                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);

                // Process lines from buffer
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(stripped) = line.strip_prefix("data:") {
                        let data = stripped.trim();
                        if data == "[DONE]" {
                            let _ = tx
                                .send(StreamChunk {
                                    delta: String::new(),
                                    reasoning_delta: None,
                                    tool_call_delta: None,
                                    is_finished: true,
                                    usage: None,
                                })
                                .await;
                            return;
                        }

                        if let Ok(parsed) = serde_json::from_str::<SseChunk>(data) {
                            let mut text_delta = String::new();
                            let mut reasoning_delta = None;
                            let mut is_finished = false;

                            if let Some(choice) = parsed.choices.first() {
                                if let Some(ref delta) = choice.delta {
                                    if let Some(ref c) = delta.content {
                                        text_delta.push_str(c);
                                    }
                                    if let Some(r) = delta
                                        .reasoning_content
                                        .as_ref()
                                        .or(delta.reasoning.as_ref())
                                        .or(delta.thought.as_ref())
                                    {
                                        reasoning_delta = Some(r.clone());
                                    }
                                    if let Some(ref tcs) = delta.tool_calls {
                                        for tc in tcs {
                                            let tool_call_delta = ToolCallDelta {
                                                index: tc.index,
                                                id: tc.id.clone(),
                                                name: tc
                                                    .function
                                                    .as_ref()
                                                    .and_then(|f| f.name.clone()),
                                                arguments_delta: tc
                                                    .function
                                                    .as_ref()
                                                    .and_then(|f| f.arguments.clone()),
                                            };
                                            let _ = tx
                                                .send(StreamChunk {
                                                    delta: String::new(),
                                                    reasoning_delta: None,
                                                    tool_call_delta: Some(tool_call_delta),
                                                    is_finished: false,
                                                    usage: None,
                                                })
                                                .await;
                                        }
                                    }
                                }
                                if choice.finish_reason.is_some() {
                                    is_finished = true;
                                }
                            }

                            let usage = parsed.usage.map(|u| TokenUsage {
                                input_tokens: u.prompt_tokens.unwrap_or(0),
                                output_tokens: u.completion_tokens.unwrap_or(0),
                                cache_read_tokens: None,
                                cache_write_tokens: None,
                            });

                            if (!text_delta.is_empty()
                                || reasoning_delta.is_some()
                                || is_finished
                                || usage.is_some())
                                && tx
                                    .send(StreamChunk {
                                        delta: text_delta,
                                        reasoning_delta,
                                        tool_call_delta: None,
                                        is_finished,
                                        usage,
                                    })
                                    .await
                                    .is_err()
                            {
                                // Receiver dropped
                                return;
                            }
                        }
                    }
                }
            }

            // Ensure terminal chunk sent if not already finished
            let _ = tx
                .send(StreamChunk {
                    delta: String::new(),
                    reasoning_delta: None,
                    tool_call_delta: None,
                    is_finished: true,
                    usage: None,
                })
                .await;
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openai_provider_endpoint_url_resolution() {
        let p1 = OpenAiCompatibleProvider::new(
            "test1",
            "http://localhost:11434",
            None,
            HashMap::new(),
            None,
        );
        assert_eq!(
            p1.endpoint_url(),
            "http://localhost:11434/v1/chat/completions"
        );

        let p2 = OpenAiCompatibleProvider::new(
            "test2",
            "http://localhost:11434/v1",
            None,
            HashMap::new(),
            None,
        );
        assert_eq!(
            p2.endpoint_url(),
            "http://localhost:11434/v1/chat/completions"
        );

        let p3 = OpenAiCompatibleProvider::new(
            "test3",
            "https://api.openai.com/v1/",
            None,
            HashMap::new(),
            None,
        );
        assert_eq!(
            p3.endpoint_url(),
            "https://api.openai.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_user_agent_resolution_default_and_custom() {
        let default_provider = OpenAiCompatibleProvider::new(
            "default-ua",
            "http://localhost:11434",
            None,
            HashMap::new(),
            None,
        );
        assert_eq!(default_provider.user_agent, DEFAULT_USER_AGENT);
        assert!(default_provider.user_agent.starts_with("PurrCode-AI/"));

        let custom_ua = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
            AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";
        let custom_provider = OpenAiCompatibleProvider::new(
            "custom-ua",
            "http://localhost:11434",
            None,
            HashMap::new(),
            Some(custom_ua.to_string()),
        );
        assert_eq!(custom_provider.user_agent, custom_ua);

        // An invalid header value (control character) must resolve to the
        // honest default — not be stored verbatim while the client sends
        // no User-Agent at all (the headerless request strict WAFs reject).
        let invalid_provider = OpenAiCompatibleProvider::new(
            "invalid-ua",
            "http://localhost:11434",
            None,
            HashMap::new(),
            Some("Bad\u{1}UA".to_string()),
        );
        assert_eq!(invalid_provider.user_agent, DEFAULT_USER_AGENT);
    }

    #[test]
    fn test_sse_chunk_deserialization_reasoning_variants() {
        // 1. DeepSeek style: reasoning_content
        let ds_json = r#"{
            "id": "chat-1",
            "choices": [{
                "delta": {
                    "reasoning_content": "Pondering the solution..."
                }
            }]
        }"#;
        let ds_chunk: SseChunk = serde_json::from_str(ds_json).unwrap();
        let delta = ds_chunk.choices[0].delta.as_ref().unwrap();
        assert_eq!(
            delta.reasoning_content.as_deref(),
            Some("Pondering the solution...")
        );

        // 2. Ollama style: thought
        let ollama_json = r#"{
            "id": "chat-2",
            "choices": [{
                "delta": {
                    "thought": "Thinking step by step..."
                }
            }]
        }"#;
        let ollama_chunk: SseChunk = serde_json::from_str(ollama_json).unwrap();
        let delta2 = ollama_chunk.choices[0].delta.as_ref().unwrap();
        assert_eq!(delta2.thought.as_deref(), Some("Thinking step by step..."));
    }

    #[test]
    fn test_sse_chunk_deserialization_tool_call_delta() {
        let tc_json = r#"{
            "id": "chat-3",
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_123",
                        "function": {
                            "name": "edit",
                            "arguments": "{\"path\":\"foo.rs\"}"
                        }
                    }]
                }
            }]
        }"#;
        let tc_chunk: SseChunk = serde_json::from_str(tc_json).unwrap();
        let delta = tc_chunk.choices[0].delta.as_ref().unwrap();
        let calls = delta.tool_calls.as_ref().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].index, 0);
        assert_eq!(calls[0].id.as_deref(), Some("call_123"));
        assert_eq!(
            calls[0].function.as_ref().unwrap().name.as_deref(),
            Some("edit")
        );
        assert_eq!(
            calls[0].function.as_ref().unwrap().arguments.as_deref(),
            Some("{\"path\":\"foo.rs\"}")
        );
    }
}
