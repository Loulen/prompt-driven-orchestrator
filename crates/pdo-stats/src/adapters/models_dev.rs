//! The price source (ADR-0034): models.dev over HTTPS — the daemon's ONLY
//! outbound call outside `docker pull` and the shelled Trigger guards, strictly
//! outside the read path.

use crate::application::ports::{BoxFuture, PriceSource};
use crate::domain::price_sync::{PRICE_FETCH_MAX_BYTES, PRICE_FETCH_TIMEOUT};

/// The price source over HTTP.
pub struct HttpPriceSource;

impl PriceSource for HttpPriceSource {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(fetch_source(url))
    }
}

/// GET the price source — the single egress, strictly outside the read path
/// (ADR-0034). The daemon's ONLY outbound call outside `docker pull` and the
/// shelled Trigger guards.
///
/// **Async, mandatorily**: `reqwest::blocking` panics when invoked from inside
/// the runtime context, including from a `spawn_blocking` thread (those carry the
/// context) — `main.rs:18-20` documents this for the CLI paths. This is the
/// crate's first async reqwest call, so there is no precedent to copy.
async fn fetch_source(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(PRICE_FETCH_TIMEOUT)
        .connect_timeout(PRICE_FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("cannot build the HTTP client: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("request failed: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("source answered HTTP {status}"));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("cannot read the body: {e}"))?;
    if bytes.len() > PRICE_FETCH_MAX_BYTES {
        return Err(format!(
            "body is {} bytes, over the {PRICE_FETCH_MAX_BYTES}-byte ceiling",
            bytes.len()
        ));
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| format!("body is not UTF-8: {e}"))
}
