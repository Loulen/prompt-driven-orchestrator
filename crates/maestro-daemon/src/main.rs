use std::net::SocketAddr;
use std::time::Duration;

use anyhow::{Context, Result};
use axum::extract::ws::{Message, WebSocket};
use axum::extract::WebSocketUpgrade;
use axum::http::{header, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use clap::Parser;
use rust_embed::Embed;
use tokio::time;
use tracing::info;

const DEFAULT_PORT: u16 = 5172;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Embed)]
#[folder = "../../frontend/dist"]
struct FrontendAssets;

#[derive(Parser)]
#[command(
    name = "maestro-daemon",
    about = "Maestro daemon — pipeline orchestrator"
)]
struct Cli {
    #[arg(short, long, env = "MAESTRO_PORT", default_value_t = DEFAULT_PORT)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "maestro_daemon=info".into()),
        )
        .init();

    let cli = Cli::parse();
    let addr = SocketAddr::from(([127, 0, 0, 1], cli.port));

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .fallback(static_handler);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("failed to bind")?;

    info!("Maestro daemon listening on http://{addr}");
    axum::serve(listener, app).await.context("server error")?;
    Ok(())
}

async fn ws_handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_ws)
}

async fn handle_ws(mut socket: WebSocket) {
    info!("WebSocket client connected");
    let ready = serde_json::json!({ "type": "ready" });
    if socket
        .send(Message::Text(ready.to_string().into()))
        .await
        .is_err()
    {
        return;
    }

    let mut interval = time::interval(HEARTBEAT_INTERVAL);
    loop {
        interval.tick().await;
        let heartbeat = serde_json::json!({
            "type": "heartbeat",
            "ts": unix_timestamp_secs(),
        });
        if socket
            .send(Message::Text(heartbeat.to_string().into()))
            .await
            .is_err()
        {
            break;
        }
    }

    info!("WebSocket client disconnected");
}

fn unix_timestamp_secs() -> String {
    use std::time::SystemTime;
    let d = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = d.as_secs();
    let millis = d.subsec_millis();
    format!("{secs}.{millis:03}")
}

async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == "index.html" {
        return serve_index();
    }

    match FrontendAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => serve_index(),
    }
}

fn serve_index() -> Response {
    match FrontendAssets::get("index.html") {
        Some(content) => Html(content.data.into_owned()).into_response(),
        None => {
            if cfg!(debug_assertions) {
                Html(DEV_PLACEHOLDER).into_response()
            } else {
                (StatusCode::NOT_FOUND, "frontend assets not found").into_response()
            }
        }
    }
}

const DEV_PLACEHOLDER: &str = r#"<!DOCTYPE html>
<html>
<head><title>Maestro (dev)</title></head>
<body style="background:#0f1115;color:#e6e8eb;font-family:sans-serif;display:grid;place-items:center;height:100vh;margin:0">
<div style="text-align:center">
<h1>Maestro daemon running</h1>
<p>In dev mode, run the Vite frontend separately:<br><code>cd frontend && npm run dev</code></p>
</div>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::util::ServiceExt;

    fn app() -> Router {
        Router::new()
            .route("/ws", get(ws_handler))
            .fallback(static_handler)
    }

    #[tokio::test]
    async fn root_returns_html() {
        let resp = app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("<!DOCTYPE html>") || text.contains("<!doctype html>"));
    }

    #[tokio::test]
    async fn ws_connects_and_receives_ready() {
        use futures_util::StreamExt;
        use tokio_tungstenite::connect_async;

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app()).await.unwrap();
        });

        let url = format!("ws://{addr}/ws");
        let (mut ws, _) = connect_async(&url).await.unwrap();

        let msg = ws.next().await.unwrap().unwrap();
        let text = msg.into_text().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(parsed["type"], "ready");
    }

    #[tokio::test]
    async fn unknown_path_falls_back_to_index() {
        let resp = app()
            .oneshot(
                Request::builder()
                    .uri("/some/client/route")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
