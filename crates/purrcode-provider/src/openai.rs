use async_trait::async_trait;
use futures_util::StreamExt;
use purrcode_api::error::{PurrError, Result};
use purrcode_api::provider::{CompletionRequest, MessageRole, ModelProvider, StreamChunk, TokenUsage};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub struct OpenAiCompatibleProvider {
    pub name: String,
    pub base_url: String,
    pub api_key: Option<String>,
    pub headers: HashMap<String, String>,
    client: reqwest::Client,
}

impl OpenAiCompatibleProvider {
    pub fn new(
        name: impl Into<String>,
        base_url: impl Into<String>,
        api_key: Option<String>,
        headers: HashMap<String, String>,
    ) -> Self {
        let mut url = base_url.into();
        if url.ends_with('/') {
            url.pop();
        }
        Self {
            name: name.into(),
            base_url: url,
            api_key,
            headers,
            client: reqwest::Client::new(),
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
        true
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
                content: if m.content.is_empty() { None } else { Some(&m.content) },
                tool_calls: m.tool_calls.as_ref().map(|tcs| {
                    tcs.iter().map(|tc| ApiToolCall {
                        id: &tc.id,
                        r#type: "function",
                        function: ApiToolCallFunction {
                            name: &tc.name,
                            arguments: &tc.arguments,
                        },
                    }).collect()
                }),
                tool_call_id: m.tool_call_id.as_deref(),
            })
            .collect();

        let tools: Option<Vec<ApiTool>> = request.tools.as_ref().map(|ts| {
            ts.iter().map(|t| ApiTool {
                r#type: "function",
                function: ApiFunction {
                    name: &t.name,
                    description: &t.description,
                    parameters: &t.parameters,
                },
            }).collect()
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
            PurrError::Provider {
                provider: self.name.clone(),
                message: format!("HTTP request to {} failed: {}", url, e),
            }
        })?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            return Err(PurrError::Provider {
                provider: self.name.clone(),
                message: format!("HTTP error {}: {}", status, err_text),
            });
        }

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
                                                name: tc.function.as_ref().and_then(|f| f.name.clone()),
                                                arguments_delta: tc.function.as_ref().and_then(|f| f.arguments.clone()),
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
