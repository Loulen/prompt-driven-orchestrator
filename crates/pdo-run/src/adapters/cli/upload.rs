//! The upload of a `pdo run create` with attachments (#779, #839): its
//! multipart body, its bounded wait, and the sentences a cut or refused upload
//! prints.

use std::time::Duration;

use anyhow::{Context, Result};

use crate::adapters::http::runs::attachments::format_attachment_bytes;

/// The bounded wait of a `POST /runs` from the CLI (#839): a minute, plus ten
/// seconds per MB of attachments — generous for a slow link, finite for a
/// stalled one.
pub(super) fn upload_timeout(attachment_bytes: u64) -> Duration {
    let mb = attachment_bytes.div_ceil(1024 * 1024);
    Duration::from_secs(60 + 10 * mb)
}

/// The sentence for a `POST /runs` that never got an answer (#839): names the
/// host, what was being sent, whether the wait expired, and the two knobs a
/// remote instance usually trips on.
pub(super) fn upload_transport_error(
    url: &str,
    attachment_bytes: u64,
    timeout: Duration,
    err: &reqwest::Error,
) -> anyhow::Error {
    if attachment_bytes == 0 {
        return anyhow::anyhow!("failed to reach daemon at {url}: {err}");
    }
    let sent = format_attachment_bytes(attachment_bytes);
    let what = if err.is_timeout() {
        format!(
            "upload timed out after {}s: the daemon at {url} did not answer while {sent} of \
             attachments were being sent",
            timeout.as_secs()
        )
    } else {
        format!(
            "upload interrupted: the daemon at {url} did not answer or the connection was cut \
             while sending {sent} of attachments ({err})"
        )
    };
    anyhow::anyhow!(
        "{what}. On a remote instance, check the reverse proxy body limit (nginx \
         `client_max_body_size`) and `max_attachments_mb` in Settings"
    )
}

/// The detail of a non-2xx answer whose body is NOT the daemon's JSON (#839):
/// it came from a reverse proxy or another intermediary, and a 413 there is
/// that layer's body limit, not `max_attachments_mb`.
pub(super) fn intermediary_refusal_detail(url: &str, status: u16, attachment_bytes: u64, raw: &str) -> String {
    let sent = format_attachment_bytes(attachment_bytes);
    if status == 413 {
        return format!(
            "the server in front of PDO at {url} refused the request body ({sent} of attachments) \
             before it reached the daemon — no PDO error body, so this is the reverse proxy's \
             limit (nginx `client_max_body_size`), not `max_attachments_mb`. Raise it or attach less"
        );
    }
    let excerpt: String = raw.trim().chars().take(200).collect();
    let excerpt = if excerpt.is_empty() {
        "empty body".to_string()
    } else {
        excerpt
    };
    format!(
        "no PDO error body, the reply came from a proxy or another intermediary in front of the \
         daemon at {url}: {excerpt}"
    )
}

/// The multipart body of a `pdo run create` carrying attachments (#779). Text
/// fields mirror the JSON body one-to-one: strings verbatim, everything else
/// (`variables`, `skills`, `auto_name`, …) serialised the way the daemon's
/// multipart parser reads them back — JSON text for objects/lists, `true`/
/// `false` for bools.
pub(super) fn run_create_multipart(
    body: &serde_json::Map<String, serde_json::Value>,
    images: &[std::path::PathBuf],
    files: &[std::path::PathBuf],
) -> Result<(reqwest::blocking::multipart::Form, u64)> {
    let mut attachment_bytes: u64 = 0;
    let mut form = reqwest::blocking::multipart::Form::new();
    for (key, value) in body {
        let text = match value {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        form = form.text(key.clone(), text);
    }
    for (field, paths) in [("images", images), ("files", files)] {
        for path in paths {
            let data = std::fs::read(path).with_context(|| {
                format!(
                    "failed to read --{} {}",
                    field.trim_end_matches('s'),
                    path.display()
                )
            })?;
            let filename = path
                .file_name()
                .and_then(|n| n.to_str())
                .with_context(|| {
                    format!(
                        "--{} {} has no usable filename",
                        field.trim_end_matches('s'),
                        path.display()
                    )
                })?
                .to_string();
            attachment_bytes += data.len() as u64;
            let part = reqwest::blocking::multipart::Part::bytes(data).file_name(filename);
            form = form.part(field, part);
        }
    }
    Ok((form, attachment_bytes))
}
