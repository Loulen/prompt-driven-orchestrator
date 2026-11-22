//! The application banner of the test extension (`app/banner`, API 0.31):
//! the core's fixture for a banner that decides alone whether to show.
//!
//! - `GET /ext/test/banner`: `{"shown": bool}`, `false` until a test sets it,
//!   so a daemon of the test extension draws no banner by default and its
//!   layout stays the one without contribution;
//! - `PUT /ext/test/banner` (`{"shown": bool}`): sets it; the interface reads
//!   it when it loads (reload the page).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use pdo::extension_api::axum::routing::get;
use pdo::extension_api::axum::{Json, Router};
use serde_json::{json, Value};

/// Whether the banner shows, shared by its two routes.
#[derive(Clone, Default)]
pub struct AppBanner(Arc<AtomicBool>);

impl AppBanner {
    pub fn shown(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }

    pub fn set(&self, shown: bool) {
        self.0.store(shown, Ordering::Relaxed);
    }
}

pub(super) fn routes(banner: AppBanner) -> Router {
    let (read, write) = (banner.clone(), banner);
    Router::new().route(
        "/banner",
        get(move || {
            let shown = read.shown();
            async move { Json(json!({ "shown": shown })) }
        })
        .put(move |Json(body): Json<Value>| {
            let shown = body["shown"].as_bool().unwrap_or(false);
            write.set(shown);
            async move { Json(json!({ "shown": shown })) }
        }),
    )
}
