//! `GET /runs/{run_id}/artifact`: one artifact a node wrote, served from the
//! Run's live worktree or, once archived, from the durable store (ADR-0020),
//! never outside its artifacts directory. `download` hands it over as a file
//! with a readable name; `GET /runs/{run_id}/artifact/zip` zips a port folder.

use std::io::Write;
use std::path::{Path, PathBuf};

use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;

use crate::application::events::load_projected;
use crate::application::paths::archived_or_live_artifacts_dir;
use crate::Runs;

#[derive(Deserialize)]
pub struct ArtifactQuery {
    pub path: String,
    /// Serve the bytes as an attachment (`Content-Disposition`), whatever the
    /// format, under the name [`download_name`] gives.
    #[serde(default)]
    pub download: bool,
}

#[derive(Deserialize)]
pub struct ArtifactZipQuery {
    /// The port folder to zip, relative to the artifacts directory.
    pub path: String,
}

impl Runs {
    /// `GET /runs/{run_id}/artifact?path=…`: one artifact of the Run, from its
    /// live worktree or, once archived, from the durable store.
    pub async fn artifact(&self, run_id: &str, query: ArtifactQuery) -> Response {
        let resolved = match self.resolve_artifact(run_id, &query.path).await {
            Ok(p) => p,
            Err(resp) => return *resp,
        };
        if !resolved.is_file() {
            return (StatusCode::NOT_FOUND, "artifact not found").into_response();
        }

        if query.download {
            // An attachment is saved, never rendered: the bytes go out as an
            // opaque stream, an `.html` included (ADR-0028 holds).
            return match std::fs::read(&resolved) {
                Ok(bytes) => (
                    StatusCode::OK,
                    [
                        (
                            header::CONTENT_TYPE,
                            HeaderValue::from_static("application/octet-stream"),
                        ),
                        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
                        (
                            header::X_CONTENT_TYPE_OPTIONS,
                            HeaderValue::from_static("nosniff"),
                        ),
                        (
                            header::CONTENT_DISPOSITION,
                            attachment(&download_name(&query.path)),
                        ),
                    ],
                    bytes,
                )
                    .into_response(),
                Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
            };
        }

        let mime = match resolved
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            Some("png") => "image/png",
            Some("jpg" | "jpeg") => "image/jpeg",
            Some("webp") => "image/webp",
            Some("gif") => "image/gif",
            // An `.html` artifact DELIBERATELY falls through to `text/markdown`. Do NOT
            // add `Some("html") => "text/html"` (ADR-0028): serving agent-authored HTML as
            // a top-level document at the daemon origin — no auth, no CSP — would let it
            // execute arbitrary script. It is only ever rendered in a sandboxed iframe.
            _ => "text/markdown",
        };

        // #796: an artifact URL is stable across a same-iter re-spawn (same run,
        // node, iter and path) while its bytes change under it. Without a validator
        // the browser's heuristic cache may resurface the previous session's report
        // or screenshot; `no-store` makes every reload show the current bytes.
        // `nosniff`: a port may hold any format, and the browser must keep to
        // the type given here rather than guess an executable one.
        let headers = [
            (header::CONTENT_TYPE, mime),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ];
        if mime.starts_with("image/") {
            match std::fs::read(&resolved) {
                Ok(bytes) => (StatusCode::OK, headers, bytes).into_response(),
                Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
            }
        } else {
            match std::fs::read_to_string(&resolved) {
                Ok(content) => (StatusCode::OK, headers, content).into_response(),
                Err(_) => (StatusCode::NOT_FOUND, "artifact not found").into_response(),
            }
        }
    }

    /// `GET /runs/{run_id}/artifact/zip?path=…`: every regular file of a port
    /// folder in one zip, named after the node and the port.
    pub async fn artifact_zip(&self, run_id: &str, query: ArtifactZipQuery) -> Response {
        let resolved = match self.resolve_artifact(run_id, &query.path).await {
            Ok(p) => p,
            Err(resp) => return *resp,
        };
        if !resolved.is_dir() {
            return (StatusCode::NOT_FOUND, "port folder not found").into_response();
        }
        match zip_folder(&resolved) {
            Ok(bytes) => (
                StatusCode::OK,
                [
                    (
                        header::CONTENT_TYPE,
                        HeaderValue::from_static("application/zip"),
                    ),
                    (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
                    (
                        header::X_CONTENT_TYPE_OPTIONS,
                        HeaderValue::from_static("nosniff"),
                    ),
                    (
                        header::CONTENT_DISPOSITION,
                        attachment(&format!("{}.zip", readable_stem(&query.path))),
                    ),
                ],
                bytes,
            )
                .into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("could not zip the port folder: {e}"),
            )
                .into_response(),
        }
    }

    /// `path` inside the Run's artifacts directory — the durable store for an
    /// archived run (worktree gone), the live worktree otherwise — refused when
    /// it leaves it.
    async fn resolve_artifact(&self, run_id: &str, path: &str) -> Result<PathBuf, Box<Response>> {
        let (_, run_state) = load_projected(self, run_id).await?;
        let artifacts_dir = archived_or_live_artifacts_dir(self, &run_state, run_id);

        let resolved = artifacts_dir
            .join(Path::new(path))
            .canonicalize()
            .map_err(|_| Box::new((StatusCode::NOT_FOUND, "artifact not found").into_response()))?;
        let canonical_artifacts = artifacts_dir.canonicalize().map_err(|_| {
            Box::new((StatusCode::NOT_FOUND, "artifacts directory not found").into_response())
        })?;
        if !resolved.starts_with(&canonical_artifacts) {
            return Err(Box::new(
                (StatusCode::BAD_REQUEST, "path traversal not allowed").into_response(),
            ));
        }
        Ok(resolved)
    }
}

/// The name a downloaded artifact is saved under. A port's `output.md` or
/// `output.html` is named after its node and port
/// (`planner/iter-2/plan/output.md` -> `planner-plan-iter-2.md`); any other
/// file keeps its own name.
pub fn download_name(path: &str) -> String {
    let file = path.rsplit('/').next().unwrap_or(path);
    match file.strip_prefix("output.") {
        Some(ext @ ("md" | "html")) if path.contains('/') => {
            let folder = &path[..path.len() - file.len() - 1];
            format!("{}.{ext}", readable_stem(folder))
        }
        _ => file.to_string(),
    }
}

/// A port folder's name for a human: `<node>-<port>`, with `-iter-<n>` past the
/// first iteration; the folder's own name for any other layout.
pub fn readable_stem(folder: &str) -> String {
    let parts: Vec<&str> = folder.trim_matches('/').split('/').collect();
    match parts.as_slice() {
        [node, iter, port] if iter.starts_with("iter-") => {
            if *iter == "iter-1" {
                format!("{node}-{port}")
            } else {
                format!("{node}-{port}-{iter}")
            }
        }
        _ => parts.last().copied().unwrap_or("artifact").to_string(),
    }
}

/// `attachment; filename="…"`, the name stripped of what would break the
/// header (quotes, control characters) and spelled in UTF-8 too (RFC 6266).
fn attachment(name: &str) -> HeaderValue {
    let ascii: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_graphic() && c != '"' && c != '\\' || c == ' ' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let encoded: String = name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    HeaderValue::from_str(&format!(
        "attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"
    ))
    .unwrap_or_else(|_| HeaderValue::from_static("attachment"))
}

/// The regular files directly in `dir`, sorted by name, in a deflated zip.
fn zip_folder(dir: &Path) -> std::io::Result<Vec<u8>> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|ft| ft.is_file()))
        .map(|e| e.path())
        .collect();
    entries.sort();

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for path in entries {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        zip.start_file(name, options)
            .map_err(std::io::Error::other)?;
        zip.write_all(&std::fs::read(&path)?)?;
    }
    let cursor = zip.finish().map_err(std::io::Error::other)?;
    Ok(cursor.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_port_report_is_named_after_its_node_and_port() {
        assert_eq!(
            download_name("planner/iter-1/plan/output.md"),
            "planner-plan.md"
        );
        assert_eq!(
            download_name("planner/iter-3/page/output.html"),
            "planner-page-iter-3.html"
        );
    }

    #[test]
    fn a_dropped_file_keeps_its_own_name() {
        assert_eq!(
            download_name("designer/iter-1/mockups/cart.pdf"),
            "cart.pdf"
        );
        assert_eq!(download_name("_input/brief.csv"), "brief.csv");
        assert_eq!(
            download_name("designer/iter-1/exports/output.csv"),
            "output.csv"
        );
    }

    #[test]
    fn the_run_prompt_is_named_after_its_folder() {
        assert_eq!(download_name("_input/output.md"), "_input.md");
    }

    #[test]
    fn a_port_folder_zip_is_named_after_its_node_and_port() {
        assert_eq!(readable_stem("designer/iter-1/mockups"), "designer-mockups");
        assert_eq!(
            readable_stem("designer/iter-2/mockups/"),
            "designer-mockups-iter-2"
        );
    }

    #[test]
    fn an_attachment_name_never_breaks_the_header() {
        let value = attachment("ré\"sumé.pdf");
        let text = value.to_str().unwrap();
        assert!(
            text.starts_with("attachment; filename=\"r__sum_.pdf\""),
            "{text}"
        );
        assert!(
            text.contains("filename*=UTF-8''r%C3%A9%22sum%C3%A9.pdf"),
            "{text}"
        );
    }
}
