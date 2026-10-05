//! `GET /fs/browse` — read-only, single-level filesystem explorer. See
//! [`crate::domain::browse`] for what it exposes and why.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use pdo_kernel::Actor;
use serde::Deserialize;

use crate::domain::browse::BrowseRootError;

use super::{error as error_body, Http, ProjectSettings};

#[derive(Deserialize)]
pub(super) struct BrowseQuery {
    /// Absolute directory to list. Omitted → the default chain (`$HOME → repo_root →
    /// /`). The frontend only ever sends absolute paths; a relative one is a caller
    /// bug (→ 400).
    #[serde(default)]
    path: Option<String>,
    /// List regular files alongside the navigable directories.
    ///
    /// **Wire vocabulary is strict**: axum's `Query` goes through `serde_urlencoded` →
    /// `str::parse::<bool>()`, so only the lowercase literals `true` / `false` are
    /// accepted. A valueless `?files`, `?files=1` or `?files=TRUE` is a `400`, NOT a
    /// silent `false`.
    #[serde(default)]
    files: bool,
    /// Show dot-entries (`.claude`, `.pdo`, …). Needed by the Dockerfile picker, whose
    /// default lives at `~/.pdo/sandbox/Dockerfile`. Same strict wire vocabulary as
    /// [`BrowseQuery::files`].
    #[serde(default)]
    hidden: bool,
}

/// `GET /fs/browse`: `403` when the caller may not browse the disk (CORE-5).
pub(super) async fn browse<S: ProjectSettings>(
    State(http): State<Http<S>>,
    Extension(actor): Extension<Actor>,
    Query(q): Query<BrowseQuery>,
) -> Response {
    if let Err(denied) = http.workspace.authorize_browse(&actor) {
        return denied.into_response();
    }
    match http.workspace.browse(q.path.as_deref(), q.files, q.hidden) {
        Ok(listing) => Json(serde_json::json!({
            "path": listing.path,
            "parent": listing.parent,
            "entries": listing.entries,
            "truncated": listing.truncated,
            "error": listing.error,
        }))
        .into_response(),
        Err(BrowseRootError::Relative) => {
            error_body(StatusCode::BAD_REQUEST, "path must be an absolute path")
        }
        Err(BrowseRootError::NoDefault) => error_body(
            StatusCode::INTERNAL_SERVER_ERROR,
            "could not resolve a default browse directory",
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::Query;

    /// Deserialise a query string through the PRODUCTION extractor (`Query`'s own
    /// `try_from_uri`, public in axum 0.8) — no HTTP, no router, but the exact
    /// `serde_urlencoded` path a real request takes.
    fn browse_query(query: &str) -> Result<BrowseQuery, axum::extract::rejection::QueryRejection> {
        let uri: axum::http::Uri = format!("/fs/browse{query}").parse().unwrap();
        Query::<BrowseQuery>::try_from_uri(&uri).map(|Query(q)| q)
    }

    #[test]
    fn browse_query_defaults_are_dirs_only_hidden_off() {
        // The load-bearing default: no query string ⇒ the pre-#431 listing, bit for bit.
        let q = browse_query("").unwrap();
        assert_eq!(q.path, None);
        assert!(!q.files, "files must default to false (dirs only)");
        assert!(
            !q.hidden,
            "hidden must default to false (dot-entries filtered)"
        );
    }

    #[test]
    fn browse_query_accepts_true_and_false() {
        let q = browse_query("?path=/tmp&files=true&hidden=true").unwrap();
        assert_eq!(q.path.as_deref(), Some("/tmp"));
        assert!(q.files);
        assert!(q.hidden);

        let q = browse_query("?files=false&hidden=false").unwrap();
        assert!(!q.files);
        assert!(!q.hidden);
    }

    #[test]
    fn browse_query_rejects_valueless_and_non_literal_flags() {
        // Pins the STRICT wire vocabulary: `serde_urlencoded` defers to
        // `str::parse::<bool>()`, so anything but the lowercase literals is a 400 —
        // never a silent `false`. Documented on `BrowseQuery` so nobody discovers this
        // by hand.
        for q in [
            "?files",
            "?files=1",
            "?files=0",
            "?files=yes",
            "?files=TRUE",
            "?hidden",
            "?hidden=yes",
        ] {
            assert!(
                browse_query(q).is_err(),
                "`{q}` must be rejected, not coerced"
            );
        }
    }
}
