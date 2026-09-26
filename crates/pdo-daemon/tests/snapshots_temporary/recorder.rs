//! The recorder of the temporary snapshot suite: it plays a scenario's requests
//! against a real daemon, keeps every response and every `/ws` message, masks
//! them ([`super::mask`]) and freezes the whole transcript as one `insta` JSON
//! snapshot.
//!
//! WebSocket messages are frozen as a **sorted list**, not in arrival order: two
//! tasks of the daemon may broadcast in either order, and the snapshot freezes
//! the JSON of each message, not the scheduler's interleaving. Heartbeats (a
//! clock) and stray events ([`super::mask::STRAY_EVENT_KINDS`]) are left out.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::StreamExt;
use reqwest::Method;
use serde_json::Value;

use super::mask::Masker;
use crate::common::{ws_text, TestDaemon};

pub struct Recorder<'a> {
    daemon: &'a TestDaemon,
    client: reqwest::Client,
    pub mask: Masker,
    http: Vec<Value>,
    ws: Arc<Mutex<Vec<String>>>,
    ws_task: tokio::task::JoinHandle<()>,
}

impl<'a> Recorder<'a> {
    pub async fn start(daemon: &'a TestDaemon) -> Self {
        let mut mask = Masker::default();
        mask.fixed(daemon.repo_root().to_string_lossy(), "[repo]");
        if let Ok(canonical) = daemon.repo_root().canonicalize() {
            mask.fixed(canonical.to_string_lossy(), "[repo]");
        }
        mask.fixed(daemon.addr.to_string(), "[addr]");
        mask.fixed(daemon.tmux_socket(), "pdo-[port]");
        if let Ok(home) = std::env::var("HOME") {
            mask.fixed(home, "[home]");
        }

        let mut stream = daemon.connect_ws().await.expect("connect /ws");
        let ws = Arc::new(Mutex::new(Vec::new()));
        let sink = ws.clone();
        let ws_task = tokio::spawn(async move {
            while let Some(Ok(msg)) = stream.next().await {
                if let Some(text) = ws_text(&msg) {
                    sink.lock().unwrap().push(text.to_string());
                }
            }
        });
        // The `ready` greeting: the subscription is live before the first request.
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while ws.lock().unwrap().is_empty() {
            assert!(std::time::Instant::now() < deadline, "no ready on /ws");
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        Self {
            daemon,
            client: reqwest::Client::new(),
            mask,
            http: Vec::new(),
            ws,
            ws_task,
        }
    }

    /// Play one request and freeze it. Answers the status and the **unmasked**
    /// body, for the scenario to read ids from.
    pub async fn call(&mut self, method: Method, path: &str, body: Option<Value>) -> (u16, Value) {
        let mut req = self
            .client
            .request(method.clone(), format!("{}{path}", self.daemon.url()));
        match &body {
            // A plain-text body (a skill's reference file) travels as is.
            Some(Value::String(text)) => req = req.body(text.clone()),
            Some(body) => req = req.json(body),
            None => {}
        }
        let resp = req.send().await.expect("request reaches the daemon");
        let status = resp.status().as_u16();
        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let text = resp.text().await.unwrap_or_default();
        let parsed = if content_type.starts_with("application/json") {
            serde_json::from_str(&text).unwrap_or(Value::String(text.clone()))
        } else if text.is_empty() {
            Value::Null
        } else {
            Value::String(text.clone())
        };

        // Kept raw: masking waits for `finish`, when every alias the scenario
        // names (even after the response that first carries it) is known.
        let mut entry = serde_json::Map::new();
        entry.insert("request".into(), Value::String(format!("{method} {path}")));
        if let Some(body) = &body {
            entry.insert("body".into(), body.clone());
        }
        entry.insert("status".into(), Value::from(status));
        if !content_type.is_empty() {
            entry.insert("content_type".into(), Value::String(content_type));
        }
        entry.insert("response".into(), parsed.clone());
        self.http.push(Value::Object(entry));
        (status, parsed)
    }

    pub async fn get(&mut self, path: &str) -> (u16, Value) {
        self.call(Method::GET, path, None).await
    }

    pub async fn post(&mut self, path: &str, body: Value) -> (u16, Value) {
        self.call(Method::POST, path, Some(body)).await
    }

    pub async fn put(&mut self, path: &str, body: Value) -> (u16, Value) {
        self.call(Method::PUT, path, Some(body)).await
    }

    pub async fn patch(&mut self, path: &str, body: Value) -> (u16, Value) {
        self.call(Method::PATCH, path, Some(body)).await
    }

    pub async fn delete(&mut self, path: &str) -> (u16, Value) {
        self.call(Method::DELETE, path, None).await
    }

    /// Name an id the scenario created, so every later occurrence reads
    /// `[kind-N]` in the snapshot.
    pub fn alias(&mut self, token: &str, kind: &'static str) {
        self.mask.alias(token, kind);
    }

    /// Wait until `/ws` stays quiet, then freeze the transcript under `name`.
    pub async fn finish(mut self, name: &str) {
        let mut seen = 0;
        let mut quiet_since = std::time::Instant::now();
        let deadline = quiet_since + Duration::from_secs(20);
        loop {
            let len = self.ws.lock().unwrap().len();
            if len != seen {
                seen = len;
                quiet_since = std::time::Instant::now();
            } else if quiet_since.elapsed() >= Duration::from_millis(800) {
                break;
            }
            assert!(std::time::Instant::now() < deadline, "/ws never went quiet");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        self.ws_task.abort();

        let http: Vec<Value> = self.http.iter().map(|e| self.mask.value(e)).collect();
        let raw: Vec<String> = self.ws.lock().unwrap().clone();
        let mut ws: Vec<(String, Value)> = raw
            .iter()
            .filter_map(|text| serde_json::from_str::<Value>(text).ok())
            .filter(|v| v["type"] != "heartbeat" && v["type"] != "ready")
            .filter(|v| !super::mask::is_stray_event(&v["event"]))
            .map(|v| {
                let masked = self.mask.value(&v);
                (masked.to_string(), masked)
            })
            .collect();
        ws.sort_by(|a, b| a.0.cmp(&b.0));

        let transcript = serde_json::json!({
            "http": http,
            "ws": ws.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        });
        let mut settings = insta::Settings::clone_current();
        settings.set_snapshot_path("snapshots");
        settings.set_prepend_module_to_snapshot(false);
        settings.set_omit_expression(true);
        settings.set_description(
            "TEMPORARY — refactor net of spec #913 (issue #914), deleted by #932.",
        );
        settings.bind(|| insta::assert_json_snapshot!(name, transcript));
    }
}
