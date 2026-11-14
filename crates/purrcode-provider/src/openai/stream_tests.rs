// SPDX-License-Identifier: GPL-3.0-or-later

//! Stream-truncation signal tests for the OpenAI-compatible provider.
//! Kept in a separate file so `openai.rs` stays under the ADR-011
//! 800-line limit (the two tests below are what pushed it over in the
//! v0.3 polish).

use super::*;
use purrcode_api::provider::ChatMessage;

/// A close-delimited SSE body that ends without finish_reason /
/// [DONE] (the shape a flaky proxy/upstream truncation produces)
/// must surface `stream_error` — never pass as a complete answer.
#[tokio::test]
async fn test_truncated_close_delimited_stream_signals_stream_error() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = socket.read(&mut buf).await;
        let _ = socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Partial\"},\"finish_reason\":null}]}\n\n",
            )
            .await;
        // Close without [DONE] / finish_reason: the truncated shape.
        drop(socket);
    });

    let provider = OpenAiCompatibleProvider::new(
        "trunc-test",
        format!("http://{addr}"),
        None,
        HashMap::new(),
        None,
    )
    .unwrap();
    let request = CompletionRequest {
        model: "m".to_string(),
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "hi".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: None,
        temperature: None,
        max_tokens: None,
        reasoning_effort: None,
        stop: None,
    };

    let mut rx = provider.stream_completion(request).await.unwrap();
    let mut saw_partial_text = false;
    let mut saw_error = false;
    while let Some(chunk) = rx.recv().await {
        if chunk.delta.contains("Partial") {
            saw_partial_text = true;
        }
        if chunk.stream_error.is_some() {
            saw_error = true;
        }
        if chunk.is_finished {
            break;
        }
    }
    assert!(saw_partial_text, "partial text should still arrive");
    assert!(
        saw_error,
        "a stream cut short must be signalled, not passed as complete"
    );
}

/// The same server shape with a real finish must NOT signal an error.
#[tokio::test]
async fn test_clean_stream_has_no_stream_error() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = vec![0u8; 8192];
        let _ = socket.read(&mut buf).await;
        let _ = socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\n\r\ndata: {\"choices\":[{\"delta\":{\"content\":\"Hello\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n",
            )
            .await;
        drop(socket);
    });

    let provider = OpenAiCompatibleProvider::new(
        "clean-test",
        format!("http://{addr}"),
        None,
        HashMap::new(),
        None,
    )
    .unwrap();
    let request = CompletionRequest {
        model: "m".to_string(),
        messages: vec![ChatMessage {
            role: MessageRole::User,
            content: "hi".to_string(),
            tool_calls: None,
            tool_call_id: None,
        }],
        tools: None,
        temperature: None,
        max_tokens: None,
        reasoning_effort: None,
        stop: None,
    };

    let mut rx = provider.stream_completion(request).await.unwrap();
    let mut text = String::new();
    let mut saw_error = false;
    while let Some(chunk) = rx.recv().await {
        if chunk.stream_error.is_some() {
            saw_error = true;
        }
        text.push_str(&chunk.delta);
        if chunk.is_finished {
            break;
        }
    }
    assert_eq!(text, "Hello");
    assert!(!saw_error, "a finished stream must not signal an error");
}
