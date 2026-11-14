// SPDX-License-Identifier: GPL-3.0-or-later

//! Shared helpers for the WebUI server tests (`tests.rs`,
//! `tests_plugins.rs`): a throwaway server on a free port, raw
//! HTTP helpers, and the recording dispatcher.

use super::*;
use purrcode_api::plugin::PluginRegistry;
use purrcode_api::tool::AuditSink;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

pub(crate) fn free_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

pub(crate) fn test_dir(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("purrcode-web-test-{}-{}", std::process::id(), name));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub(crate) async fn start_server(
    token: Option<String>,
    purrcode_dir: PathBuf,
    dispatcher: Option<Arc<dyn PromptDispatcher>>,
) -> (
    u16,
    Arc<WebSessionRegistry>,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    start_server_with_registry(token, purrcode_dir, dispatcher, None, None).await
}

/// Same as `start_server`, plus an optional plugin registry and
/// audit sink for the mount/unmount endpoint tests.
pub(crate) async fn start_server_with_registry(
    token: Option<String>,
    purrcode_dir: PathBuf,
    dispatcher: Option<Arc<dyn PromptDispatcher>>,
    plugin_registry: Option<Arc<dyn PluginRegistry>>,
    audit_sink: Option<Arc<dyn AuditSink>>,
) -> (
    u16,
    Arc<WebSessionRegistry>,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    start_server_with_skill_registry(
        token,
        purrcode_dir,
        dispatcher,
        plugin_registry,
        None,
        audit_sink,
    )
    .await
}

/// Full builder: plugin registry, skill registry, and audit sink.
/// The skill endpoints (B5) answer 503 without a registry, so
/// tests that exercise them pass one in; the memory endpoints
/// need no registry and are tested through this helper too.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn start_server_with_skill_registry(
    token: Option<String>,
    purrcode_dir: PathBuf,
    dispatcher: Option<Arc<dyn PromptDispatcher>>,
    plugin_registry: Option<Arc<dyn PluginRegistry>>,
    skill_registry: Option<Arc<dyn purrcode_api::skill::SkillRegistry>>,
    audit_sink: Option<Arc<dyn AuditSink>>,
) -> (
    u16,
    Arc<WebSessionRegistry>,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let port = free_port();
    let registry = Arc::new(WebSessionRegistry::new(purrcode_dir.clone()));
    let mut server = WebServer::new(
        "127.0.0.1",
        port,
        token,
        registry.clone(),
        purrcode_dir,
        PathBuf::from("/tmp/ws"),
    );
    if let Some(disp) = dispatcher {
        server = server.with_dispatcher(disp);
    }
    if let Some(reg) = plugin_registry {
        server = server.with_plugin_registry(reg);
    }
    if let Some(reg) = skill_registry {
        server = server.with_skill_registry(reg);
    }
    if let Some(sink) = audit_sink {
        server = server.with_audit_sink(sink);
    }
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(async move {
        let _ = server.run(cancel_rx).await;
    });
    // Give the listener a moment to bind.
    tokio::time::sleep(Duration::from_millis(100)).await;
    (port, registry, cancel_tx, handle)
}

/// Same builder, plus a cron registry (v0.4 B6): the scheduler
/// endpoints answer 503 without one, so tests that exercise them
/// pass one in.
pub(crate) async fn start_server_with_cron_registry(
    token: Option<String>,
    purrcode_dir: PathBuf,
    cron_registry: Option<Arc<dyn purrcode_api::cron::CronRegistry>>,
    audit_sink: Option<Arc<dyn AuditSink>>,
) -> (
    u16,
    Arc<WebSessionRegistry>,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
) {
    let port = free_port();
    let registry = Arc::new(WebSessionRegistry::new(purrcode_dir.clone()));
    let mut server = WebServer::new(
        "127.0.0.1",
        port,
        token,
        registry.clone(),
        purrcode_dir,
        PathBuf::from("/tmp/ws"),
    );
    if let Some(reg) = cron_registry {
        server = server.with_cron_registry(reg);
    }
    if let Some(sink) = audit_sink {
        server = server.with_audit_sink(sink);
    }
    let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
    let handle = tokio::spawn(async move {
        let _ = server.run(cancel_rx).await;
    });
    // Give the listener a moment to bind.
    tokio::time::sleep(Duration::from_millis(100)).await;
    (port, registry, cancel_tx, handle)
}

/// A dispatcher that records (session_id, prompt) instead of
/// running a turn — the B1 prompt-routing tests assert the server
/// picked the right session.
pub(crate) struct RecordingDispatcher {
    pub(crate) calls: Arc<std::sync::Mutex<Vec<(String, String)>>>,
}

#[async_trait::async_trait]
impl PromptDispatcher for RecordingDispatcher {
    async fn dispatch(&self, session: Arc<WebSession>, prompt: String) -> Result<()> {
        self.calls
            .lock()
            .unwrap()
            .push((session.id.clone(), prompt));
        Ok(())
    }
}

/// One-shot raw HTTP request. Sends `Connection: close` so the
/// server (Axum/hyper, keep-alive by default since B2) closes the
/// connection after the response and `read_to_string` returns
/// promptly instead of waiting out the 5s read timeout.
pub(crate) fn http_raw(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut buf = String::new();
    let _ = stream.read_to_string(&mut buf);
    buf
}

/// Read from an SSE stream until a `data:` line arrives (or the
/// read times out). Returns everything read so far.
pub(crate) fn http_sse_until_data(port: u16) -> String {
    http_sse_read(port, "/api/events", Duration::from_secs(5))
}

/// Same, for an arbitrary SSE path and timeout (the B1 isolation
/// tests subscribe per session and assert the *other* session gets
/// nothing within a short window).
pub(crate) fn http_sse_read(port: u16, path: &str, timeout: Duration) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.set_read_timeout(Some(timeout)).unwrap();
    stream
        .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
        .unwrap();
    let mut out = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                out.extend_from_slice(&chunk[..n]);
                if out.windows(5).any(|w| w == b"data:") {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

pub(crate) fn get(port: u16, path: &str) -> String {
    http_raw(
        port,
        &format!(
            "GET {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n",
            path
        ),
    )
}

pub(crate) fn post(port: u16, path: &str, body: &str) -> String {
    http_raw(
            port,
            &format!(
                "POST {} HTTP/1.1\r\nHost: x\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                path,
                body.len(),
                body
            ),
        )
}
