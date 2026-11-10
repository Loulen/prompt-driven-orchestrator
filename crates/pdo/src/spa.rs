//! The embedded SPA, served as the fallback of the router. Every path no route
//! claims gets `index.html`, so the front's own routes deep-link.
//!
//! `build.rs` puts the built frontend in `$OUT_DIR/frontend-dist` (from
//! `PDO_FRONTEND_DIST` or `pnpm run build`) and exports that folder, relative to
//! this crate, as `PDO_FRONTEND_EMBED_DIR`: the `compression` feature of
//! rust-embed refuses an absolute `folder`. Release builds compile the files in;
//! debug builds read them from that folder at runtime (no `debug-embed`).
//!
//! Caching: a file of `assets/` carries the hash of its content in its name
//! (Vite), so the browser keeps it for good (`immutable`); everything else
//! (`index.html`, which names those files, and the unhashed shared modules of
//! the extensions) is revalidated on every load (`no-cache`), so an update
//! reaches the browser without emptying its cache.
//!
//! The Marque of the binary ([`Brand`]) is written into `index.html` (its title
//! and its `data-brand`) and its logos are served under `/shared/brand/`, ahead
//! of the embedded files.

use axum::http::{header, Method, StatusCode, Uri};
use axum::response::{Html, IntoResponse, Response};
use rust_embed::Embed;

use crate::brand::Brand;

#[derive(Embed)]
#[folder = "$PDO_FRONTEND_EMBED_DIR"]
struct FrontendAssets;

/// The folder Vite writes its hashed files to.
const HASHED_DIR: &str = "assets/";

/// A file whose name changes with its content: kept for good.
const IMMUTABLE: &str = "public, max-age=31536000, immutable";

/// Any other file: revalidated on every load.
const REVALIDATE: &str = "no-cache";

/// Where the Marque's logos are served.
const BRAND_DIR: &str = "shared/brand/";

/// The favicon's address before the Marque: the pages an extension serves
/// outside the interface still link it, and get the Marque's.
const LEGACY_FAVICON: &str = "shared/favicon.svg";

/// The router's fallback: the embedded SPA under `brand`.
pub(crate) fn static_handler(
    brand: Brand,
) -> impl Fn(Method, Uri) -> std::future::Ready<Response> + Clone + Send + Sync + 'static {
    move |method, uri| {
        std::future::ready(serve(brand, method, &uri, |path| {
            FrontendAssets::get(path).map(|content| content.data.into_owned())
        }))
    }
}

/// The answer to `uri`, the files read through `file`.
fn serve(
    brand: Brand,
    method: Method,
    uri: &Uri,
    file: impl Fn(&str) -> Option<Vec<u8>>,
) -> Response {
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::NOT_FOUND.into_response();
    }

    let path = uri.path().trim_start_matches('/');

    if path.is_empty() || path == "index.html" {
        return serve_index(brand, &file);
    }

    let path = if path == LEGACY_FAVICON {
        "shared/brand/favicon.svg"
    } else {
        path
    };
    if let Some(name) = path.strip_prefix(BRAND_DIR) {
        return match brand.asset(name) {
            Some(svg) => (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "image/svg+xml"),
                    (header::CACHE_CONTROL, REVALIDATE),
                ],
                svg,
            )
                .into_response(),
            None => StatusCode::NOT_FOUND.into_response(),
        };
    }

    match file(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, mime.as_ref()),
                    (header::CACHE_CONTROL, cache_control(path)),
                ],
                content,
            )
                .into_response()
        }
        None => serve_index(brand, &file),
    }
}

/// The `Cache-Control` of the file `path`.
fn cache_control(path: &str) -> &'static str {
    if path.starts_with(HASHED_DIR) {
        IMMUTABLE
    } else {
        REVALIDATE
    }
}

fn serve_index(brand: Brand, file: &impl Fn(&str) -> Option<Vec<u8>>) -> Response {
    let page = match file("index.html") {
        Some(content) => Html(brand.stamp(&String::from_utf8_lossy(&content))).into_response(),
        None => {
            if cfg!(debug_assertions) {
                Html(brand.stamp(DEV_PLACEHOLDER)).into_response()
            } else {
                return (StatusCode::NOT_FOUND, "frontend assets not found").into_response();
            }
        }
    };
    ([(header::CACHE_CONTROL, REVALIDATE)], page).into_response()
}

const DEV_PLACEHOLDER: &str = r#"<!DOCTYPE html>
<html data-brand="pdo">
<head><title>PDO</title><link rel="icon" type="image/svg+xml" href="/shared/brand/favicon.svg" /></head>
<body style="background:#0f1115;color:#e6e8eb;font-family:sans-serif;display:grid;place-items:center;height:100vh;margin:0">
<div style="text-align:center">
<h1>The daemon is running (dev)</h1>
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
            .fallback(static_handler(Brand::Pdo))
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

    /// The SPA over two files: its page and one hashed asset.
    fn files(path: &str) -> Option<Vec<u8>> {
        match path {
            "index.html" => Some(
                b"<!doctype html><html data-brand=\"pdo\"><title>PDO</title><script src=\"/assets/index-abc123.js\">"
                    .to_vec(),
            ),
            "assets/index-abc123.js" => Some(b"export {};".to_vec()),
            "shared/react.js" => Some(b"export default {};".to_vec()),
            _ => None,
        }
    }

    fn cache_control_of(path: &str) -> String {
        let response = serve(
            Brand::Pdo,
            Method::GET,
            &path.parse::<Uri>().unwrap(),
            files,
        );
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        response.headers()[header::CACHE_CONTROL]
            .to_str()
            .unwrap()
            .to_string()
    }

    #[test]
    fn a_hashed_asset_is_kept_for_good_and_the_page_revalidated() {
        assert_eq!(
            cache_control_of("/assets/index-abc123.js"),
            "public, max-age=31536000, immutable"
        );
        assert_eq!(cache_control_of("/"), "no-cache");
        assert_eq!(cache_control_of("/index.html"), "no-cache");
        // A route of the front gets the page, revalidated like it.
        assert_eq!(cache_control_of("/runs/r1/review"), "no-cache");
        // An unhashed file keeps its name across versions.
        assert_eq!(cache_control_of("/shared/react.js"), "no-cache");
    }

    #[tokio::test]
    async fn the_embedded_page_is_revalidated() {
        let resp = spa_app()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.headers()[header::CACHE_CONTROL], "no-cache");
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

    fn get(brand: Brand, path: &str) -> Response {
        serve(brand, Method::GET, &path.parse::<Uri>().unwrap(), files)
    }

    async fn text_of(response: Response) -> String {
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8_lossy(&body).into_owned()
    }

    #[tokio::test]
    async fn the_page_carries_the_brand_on_every_route_of_the_front() {
        for path in ["/", "/index.html", "/runs/r1/review"] {
            let page = text_of(get(Brand::AcogniDriver, path)).await;
            assert!(
                page.contains("<title>Acogni Driver</title>"),
                "{path}: {page}"
            );
            assert!(
                page.contains("data-brand=\"acogni-driver\""),
                "{path}: {page}"
            );
            let page = text_of(get(Brand::Pdo, path)).await;
            assert!(page.contains("<title>PDO</title>"), "{path}: {page}");
        }
    }

    #[tokio::test]
    async fn the_brand_logos_are_served_as_svg_and_revalidated() {
        let response = get(Brand::Pdo, "/shared/brand/mark.svg");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(text_of(response).await.contains("aria-label=\"PDO\""));
        let lockup = text_of(get(Brand::AcogniDriver, "/shared/brand/lockup.svg")).await;
        assert!(lockup.contains("aria-label=\"Acogni Driver\""));
        // The favicon's former address answers the Marque's.
        let legacy = get(Brand::AcogniDriver, "/shared/favicon.svg");
        assert_eq!(legacy.headers()[header::CONTENT_TYPE], "image/svg+xml");
        assert_eq!(
            text_of(legacy).await,
            Brand::AcogniDriver.asset("favicon.svg").unwrap()
        );
        // An unknown logo is a 404, never the page.
        assert_eq!(
            get(Brand::Pdo, "/shared/brand/logo.png").status(),
            StatusCode::NOT_FOUND
        );
    }
}
