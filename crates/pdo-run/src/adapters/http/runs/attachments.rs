//! The attachments of a multipart `POST /runs` (#779, #839): the names they may
//! take, and the sentences a refused or cut upload answers with.

use std::path::Path;

use axum::http::StatusCode;

use crate::application::node::prompt::ALLOWED_IMAGE_EXTENSIONS;

/// The prompt file every Run writes into `_input/` — an attachment may not
/// take its name, it would clobber the user prompt.
pub const INPUT_PROMPT_FILENAME: &str = "output.md";

/// Sanitize the filename of a `files` attachment: keep the last path component
/// only (no traversal), refuse empty / dot names and a collision with the
/// prompt file. Every extension is accepted — the per-run size budget is the
/// guard, not a type list (#779).
pub fn sanitize_attachment_filename(raw: &str) -> Result<String, String> {
    let name = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .trim();
    if name.is_empty() || name == "." || name == ".." {
        return Err(format!("invalid attachment filename: {raw:?}"));
    }
    if name == INPUT_PROMPT_FILENAME {
        return Err(format!(
            "attachment `{name}` is reserved for the run prompt; rename the file"
        ));
    }
    Ok(name.to_string())
}

pub fn has_image_extension(name: &str) -> bool {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// A refused multipart body: the status the client gets and the sentence that
/// names why. `PAYLOAD_TOO_LARGE` for the budget, `BAD_REQUEST` otherwise.
pub struct MultipartRefusal {
    pub status: StatusCode,
    pub message: String,
}

impl From<String> for MultipartRefusal {
    fn from(message: String) -> Self {
        MultipartRefusal {
            status: StatusCode::BAD_REQUEST,
            message,
        }
    }
}

impl From<&str> for MultipartRefusal {
    fn from(message: &str) -> Self {
        MultipartRefusal::from(message.to_string())
    }
}

pub fn sanitize_image_filename(raw: &str) -> Option<String> {
    let name = Path::new(raw)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(raw);
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    if ALLOWED_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(name.to_string())
    } else {
        None
    }
}

/// The 400 sentence for a multipart body that ended before its closing
/// boundary (#839). Names the layer (transport, not the form), the attachment
/// being read when it happened, and how much had arrived, so a remote user
/// can tell a proxy body limit or a dropped connection from a client bug.
pub fn multipart_cut_message(
    last_field: Option<&str>,
    filename: Option<&str>,
    received_bytes: u64,
    cause: &str,
) -> String {
    let received = format_attachment_bytes(received_bytes);
    let where_ = match (filename, last_field) {
        (Some(name), _) if !name.is_empty() => format!("while reading attachment `{name}`"),
        (_, Some(field)) if !field.is_empty() => format!("after field `{field}`"),
        _ => "before any field".to_string(),
    };
    format!(
        "upload interrupted {where_} ({received} of attachments received): the connection was \
         cut or a proxy in front of PDO truncated the multipart body ({cause}). Check the \
         network, the reverse proxy body limit (nginx `client_max_body_size`) and \
         `max_attachments_mb` in Settings"
    )
}

/// The 400 sentence for a field that could not be read (#839). axum's
/// `MultipartError` displays a flat "Error parsing `multipart/form-data`
/// request"; the multer source underneath says whether the stream simply
/// ended (a cut upload — named as such, with the field or attachment in
/// progress) or the field itself was malformed (a plain `bad field`).
pub fn multipart_field_error(
    field: &str,
    filename: Option<&str>,
    received_bytes: u64,
    err: &axum::extract::multipart::MultipartError,
) -> String {
    let cause = std::error::Error::source(err)
        .map(|s| s.to_string())
        .unwrap_or_else(|| err.to_string());
    let lower = cause.to_lowercase();
    let cut = lower.contains("incomplete")
        || lower.contains("failed to read stream")
        || lower.contains("failed to read field complete headers");
    if cut {
        multipart_cut_message(Some(field), filename, received_bytes, &cause)
    } else {
        match filename {
            Some(name) if !name.is_empty() => {
                format!("failed to read attachment `{name}`: {cause}")
            }
            _ => format!("bad field {field}: {cause}"),
        }
    }
}

/// `18 KB`, `1.4 MB` — the size wording of upload errors (#839).
pub fn format_attachment_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{} KB", bytes / 1024)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
