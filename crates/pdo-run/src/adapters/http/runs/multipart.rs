//! The multipart body of `POST /runs` (#779): the create request's fields as
//! form parts, plus its attachments, under a per-run budget enforced on the
//! attachment parts as they arrive.

use std::collections::HashMap;

use axum::extract::Multipart;
use axum::http::StatusCode;
use pdo_skill::domain::selection::SkillRef;
use pdo_workspace::domain::provisioning;

use super::attachments::{
    has_image_extension, multipart_cut_message, multipart_field_error,
    sanitize_attachment_filename, sanitize_image_filename, MultipartRefusal,
};
use crate::application::create_run::{
    CreateRunRequest, InputFile, TargetRepoInput, CREATE_RUN_FIELDS,
};
use crate::application::stale::detector::parse_bool_setting;
use crate::domain::state::SandboxMode;

pub(super) async fn parse_multipart_create_run(
    mut multipart: Multipart,
    max_attachment_bytes: u64,
) -> std::result::Result<(CreateRunRequest, Vec<InputFile>), MultipartRefusal> {
    let mut pipeline = None;
    let mut input = None;
    let mut variables: HashMap<String, serde_yaml::Value> = HashMap::new();
    let mut pipeline_id = None;
    let mut target_repo = None;
    let mut target_repos: Vec<TargetRepoInput> = Vec::new();
    let mut source_branch = None;
    let mut name = None;
    let mut sandbox: Option<SandboxMode> = None;
    let mut harness: Option<String> = None;
    let mut agent_choice: Option<pdo_harness::agent_choice::AgentChoice> = None;
    let mut skills: Vec<SkillRef> = Vec::new();
    let mut auto_name: Option<bool> = None;
    let mut auto_fail: Option<bool> = None;
    let mut provisioning = provisioning::ProvisioningRules::default();
    let mut attachments: Vec<InputFile> = Vec::new();
    let mut attachment_bytes: u64 = 0;

    // The budget is enforced on the ATTACHMENT parts (what the user chose),
    // never on the text fields — a long prompt does not eat the file budget.
    // Refused at the first part that crosses the line, so a runaway upload is
    // cut without buffering the whole body.
    let mut push_attachment = |filename: String,
                               data: Vec<u8>,
                               attachments: &mut Vec<InputFile>|
     -> std::result::Result<(), MultipartRefusal> {
        attachment_bytes += data.len() as u64;
        if attachment_bytes > max_attachment_bytes {
            return Err(MultipartRefusal {
                status: StatusCode::PAYLOAD_TOO_LARGE,
                message: format!(
                    "attachments exceed the per-run limit of {} MB (at `{filename}`); \
                     raise `max_attachments_mb` in Settings or attach less",
                    max_attachment_bytes / (1024 * 1024)
                ),
            });
        }
        // #779: a duplicate name never overwrites — today the second silently
        // replaced the first. Named 400 so the caller can rename or drop one.
        if attachments.iter().any(|a| a.filename == filename) {
            return Err(MultipartRefusal::from(format!(
                "duplicate attachment filename `{filename}`: each attachment needs a distinct name"
            )));
        }
        let is_image = has_image_extension(&filename);
        attachments.push(InputFile {
            filename,
            data,
            is_image,
        });
        Ok(())
    };

    // #839: bytes of attachment parts fully received so far and the last field
    // seen — what a truncation error names, so a cut upload reads as a cut
    // upload and never as "missing field: input".
    let mut received_bytes: u64 = 0;
    let mut last_field: Option<String> = None;
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(field)) => field,
            Ok(None) => break,
            // A transport error mid-stream (connection reset, proxy dropped the
            // body) used to end the loop silently and fall through to a
            // misleading 400 "missing field". Named for what it is.
            Err(e) => {
                let cause = std::error::Error::source(&e)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| e.to_string());
                return Err(MultipartRefusal::from(multipart_cut_message(
                    last_field.as_deref(),
                    None,
                    received_bytes,
                    &cause,
                )));
            }
        };
        let field_name = field.name().unwrap_or("").to_string();
        last_field = Some(field_name.clone());
        match field_name.as_str() {
            "pipeline" => {
                pipeline =
                    Some(field.text().await.map_err(|e| {
                        multipart_field_error("pipeline", None, received_bytes, &e)
                    })?);
            }
            "input" => {
                input = Some(
                    field
                        .text()
                        .await
                        .map_err(|e| multipart_field_error("input", None, received_bytes, &e))?,
                );
            }
            "variables" => {
                let text = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("variables", None, received_bytes, &e))?;
                if !text.is_empty() {
                    variables = serde_json::from_str(&text)
                        .map_err(|e| format!("invalid variables JSON: {e}"))?;
                }
            }
            "pipeline_id" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("pipeline_id", None, received_bytes, &e))?;
                if !v.is_empty() {
                    pipeline_id = Some(v);
                }
            }
            "target_repo" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("target_repo", None, received_bytes, &e))?;
                if !v.is_empty() {
                    target_repo = Some(v);
                }
            }
            "target_repos" => {
                // A JSON array of {repo, base_branch?}. Empty → mono-repo; malformed
                // JSON is a 400, like `variables`.
                let text = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("target_repos", None, received_bytes, &e))?;
                if !text.is_empty() {
                    target_repos = serde_json::from_str(&text)
                        .map_err(|e| format!("invalid target_repos JSON: {e}"))?;
                }
            }
            "source_branch" => {
                let v = field.text().await.map_err(|e| {
                    multipart_field_error("source_branch", None, received_bytes, &e)
                })?;
                if !v.is_empty() {
                    source_branch = Some(v);
                }
            }
            "name" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("name", None, received_bytes, &e))?;
                if !v.is_empty() {
                    name = Some(v);
                }
            }
            "sandbox" => {
                // `parse` is syntactic, so a non-`off` token becomes `Profile(name)`
                // and an unknown NAME is a hard 400 at the chokepoint rather than a
                // silent demotion to the instance default (ADR-0031 §7).
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("sandbox", None, received_bytes, &e))?;
                if !v.is_empty() {
                    sandbox = SandboxMode::parse(&v);
                }
            }
            "harness" => {
                // No validation (ADR-0045).
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("harness", None, received_bytes, &e))?;
                if !v.trim().is_empty() {
                    harness = Some(v);
                }
            }
            "agent_choice" => {
                let value = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("agent_choice", None, received_bytes, &e))?;
                if !value.trim().is_empty() {
                    agent_choice = Some(
                        serde_json::from_str(&value)
                            .map_err(|e| format!("invalid agent_choice JSON: {e}"))?,
                    );
                }
            }
            "skills" => {
                // #669: a JSON list of `{id, name}`, mirrored from the JSON path so a
                // Run created WITH images keeps its skills.
                let value = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("skills", None, received_bytes, &e))?;
                if !value.trim().is_empty() {
                    skills = serde_json::from_str(&value)
                        .map_err(|e| format!("invalid skills JSON: {e}"))?;
                }
            }
            "auto_name" => {
                // An unrecognised token leaves `None`, deferring to the chokepoint's
                // name-presence resolution.
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("auto_name", None, received_bytes, &e))?;
                if !v.is_empty() {
                    auto_name = parse_bool_setting(&v);
                }
            }
            "auto_fail" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("auto_fail", None, received_bytes, &e))?;
                if !v.is_empty() {
                    auto_fail = parse_bool_setting(&v);
                }
            }
            "provisioning" => {
                let v = field
                    .text()
                    .await
                    .map_err(|e| multipart_field_error("provisioning", None, received_bytes, &e))?;
                provisioning =
                    serde_json::from_str(&v).map_err(|e| format!("bad field provisioning: {e}"))?;
            }
            "images" => {
                let raw_filename = field.file_name().unwrap_or("image.png").to_string();
                let data = field.bytes().await.map_err(|e| {
                    multipart_field_error("images", Some(&raw_filename), received_bytes, &e)
                })?;
                received_bytes += data.len() as u64;
                if data.is_empty() {
                    continue;
                }
                let filename = sanitize_image_filename(&raw_filename)
                    .ok_or_else(|| format!("unsupported image type: {raw_filename}"))?;
                push_attachment(filename, data.to_vec(), &mut attachments)?;
            }
            "files" => {
                // #779: any file. Extension-free, budget-guarded, name-checked.
                let raw_filename = field.file_name().unwrap_or("").to_string();
                let data = field.bytes().await.map_err(|e| {
                    multipart_field_error("files", Some(&raw_filename), received_bytes, &e)
                })?;
                received_bytes += data.len() as u64;
                if data.is_empty() {
                    continue;
                }
                let filename = sanitize_attachment_filename(&raw_filename)?;
                push_attachment(filename, data.to_vec(), &mut attachments)?;
            }
            // An unknown form field is a client mistake, named — never silently
            // dropped. `images` and `files` are the multipart-only fields; the
            // rest mirror `CREATE_RUN_FIELDS`.
            other => {
                return Err(MultipartRefusal::from(format!(
                    "unknown field `{other}` in POST /runs multipart body; accepted fields: {}, images, files",
                    CREATE_RUN_FIELDS.join(", ")
                )));
            }
        }
    }

    let req = CreateRunRequest {
        pipeline: pipeline.ok_or("missing field: pipeline")?,
        input: input.ok_or("missing field: input")?,
        variables,
        pipeline_id,
        target_repo,
        target_repos,
        source_branch,
        name,
        triggered_by: None,
        sandbox,
        harness,
        agent_choice,
        skills,
        auto_name,
        auto_fail,
        provisioning,
        // A multipart `POST /runs` is a manual launch: the form already fetched
        // before it opened, so nothing is refreshed here (#804, ADR-0070 §3).
        source_fetch_error: None,
    };
    Ok((req, attachments))
}
