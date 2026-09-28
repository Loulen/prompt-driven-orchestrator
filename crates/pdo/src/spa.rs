//! The embedded SPA: `frontend/dist`, compiled into the binary (rust-embed by
//! `build.rs`), served as the fallback of the router. Every path no route
//! claims gets `index.html`, so the front's own routes deep-link.

use axum::http::{header, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../frontend/dist"]
struct FrontendAssets;

pub(crate) async fn static_handler(method: Method, uri: Uri) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }

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
<head><title>PDO (dev)</title></head>
<body style="background:#0f1115;color:#e6e8eb;font-family:sans-serif;display:grid;place-items:center;height:100vh;margin:0">
<div style="text-align:center">
<h1>PDO daemon running</h1>
<p>In dev mode, run the Vite frontend separately:<br><code>cd frontend && pnpm run dev</code></p>
</div>
</body>
</html>"#;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::Router;
    use tower::util::ServiceExt;

    /// The SPA fallback behind a `/ws` that upgrades to nothing.
    fn spa_app() -> Router {
        Router::new()
            .route(
                "/ws",
                axum::routing::get(|ws: axum::extract::WebSocketUpgrade| async {
                    ws.on_upgrade(|_| async {})
                }),
            )
            .fallback(static_handler)
    }

    #[tokio::test]
    async fn root_returns_html() {
        let resp = spa_app()
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
    async fn unknown_path_falls_back_to_index() {
        let resp = spa_app()
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
