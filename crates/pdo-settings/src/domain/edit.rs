//! What a `PUT /settings` must satisfy before anything is stored: the rules
//! that need nothing but the body. Fail-fast — an out-of-range value is a
//! refusal, never a silent default like the env parsers.
//!
//! The rules that need the world (does the staging profile, the harness, the
//! pinned agentic profile exist?) are the use case's.

use pdo_sandbox::domain::retired_settings::{
    retired_setting_message, RETIRED_SANDBOX_IMAGE_COLUMNS,
};
use pdo_trigger::domain::guard::MAX_GUARD_TIMEOUT_SECS;

use super::instance_config::UpdateInstanceConfig;

/// The largest per-Run attachment budget, in MB: the parts are buffered
/// before the write, so the budget must stay a sane in-memory body.
pub const MAX_ATTACHMENTS_MB_LIMIT: i64 = 4096;

/// Read a raw body as an edit. It is taken as raw JSON for ONE reason: a
/// *retired* field must be refused **naming itself**, not be silently ignored
/// by serde and answered as though applied — checked before parsing, so the
/// answer is about the field the caller sent. A field nobody has ever heard of
/// stays ignored.
pub fn parse_edit(body: serde_json::Value) -> Result<UpdateInstanceConfig, String> {
    for (field, env_var) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        if body.get(field).is_some() {
            return Err(retired_setting_message(field, env_var));
        }
    }
    serde_json::from_value(body).map_err(|e| format!("invalid settings payload: {e}"))
}

/// The bounds of the numeric knobs.
pub fn check_ranges(edit: &UpdateInstanceConfig) -> Result<(), String> {
    let at_least_one = [
        (edit.session_cap, "session_cap must be >= 1"),
        (edit.reaper_ttl_secs, "reaper_ttl_secs must be >= 1"),
        (
            edit.libassist_idle_ttl_secs,
            "libassist_idle_ttl_secs must be >= 1",
        ),
    ];
    for (value, message) in at_least_one {
        if value.is_some_and(|n| n < 1) {
            return Err(message.to_string());
        }
    }
    if edit
        .guard_timeout_secs
        .is_some_and(|g| !(1..=MAX_GUARD_TIMEOUT_SECS as i64).contains(&g))
    {
        return Err(format!(
            "guard_timeout_secs must be between 1 and {MAX_GUARD_TIMEOUT_SECS} seconds"
        ));
    }
    if edit
        .max_attachments_mb
        .is_some_and(|m| !(1..=MAX_ATTACHMENTS_MB_LIMIT).contains(&m))
    {
        return Err(format!(
            "max_attachments_mb must be between 1 and {MAX_ATTACHMENTS_MB_LIMIT}"
        ));
    }
    Ok(())
}

/// The Public URL in its stored form: normalized, or `""` (the clear) for a
/// blank one; refused, naming why, when it is no `http(s)://` address.
pub fn normalize_public_url(edit: &mut UpdateInstanceConfig) -> Result<(), String> {
    if let Some(raw) = edit.public_url.take() {
        edit.public_url = Some(pdo_kernel::public_url::normalize(&raw)?.unwrap_or_default());
    }
    Ok(())
}

/// The refusal of a manager pin naming no agentic profile. A pin to nothing
/// would silently fall back at spawn time, so it is refused at registration.
pub fn unknown_manager_profile(name: &str, known: &[String]) -> String {
    format!(
        "unknown manager profile `{name}`: it names no agent profile — a pin to \
         nothing would silently fall back to « Follow the Run ». Known profiles: {}",
        known.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_retired_field_is_refused_by_name_before_anything_else() {
        for (field, env_var) in RETIRED_SANDBOX_IMAGE_COLUMNS {
            let err = parse_edit(json!({ *field: "registry", "session_cap": 5 })).unwrap_err();
            assert!(err.contains(field), "{err}");
            assert!(
                err.contains("staging profile") && err.contains(env_var),
                "{err}"
            );
        }
    }

    #[test]
    fn the_public_url_is_stored_normalized_cleared_or_refused() {
        let mut edit = parse_edit(json!({ "public_url": " https://pdo.example.net/ " })).unwrap();
        normalize_public_url(&mut edit).unwrap();
        assert_eq!(edit.public_url.as_deref(), Some("https://pdo.example.net"));
        let mut clear = parse_edit(json!({ "public_url": "" })).unwrap();
        normalize_public_url(&mut clear).unwrap();
        assert_eq!(clear.public_url.as_deref(), Some(""));
        let mut bad = parse_edit(json!({ "public_url": "pdo.example.net" })).unwrap();
        let err = normalize_public_url(&mut bad).unwrap_err();
        assert!(err.contains("http://"), "{err}");
        let mut untouched = parse_edit(json!({})).unwrap();
        normalize_public_url(&mut untouched).unwrap();
        assert_eq!(untouched.public_url, None);
    }

    #[test]
    fn an_unknown_field_is_ignored_and_a_type_error_is_named() {
        let edit = parse_edit(json!({ "session_cap": 5, "no_such_knob": 1 })).unwrap();
        assert_eq!(edit.session_cap, Some(5));
        let err = parse_edit(json!({ "session_cap": "many" })).unwrap_err();
        assert!(err.starts_with("invalid settings payload"), "{err}");
    }

    #[test]
    fn the_numeric_knobs_are_bounded() {
        let edit = |body| parse_edit(body).unwrap();
        for body in [
            json!({ "session_cap": 0 }),
            json!({ "reaper_ttl_secs": 0 }),
            json!({ "libassist_idle_ttl_secs": 0 }),
            json!({ "guard_timeout_secs": 0 }),
            json!({ "guard_timeout_secs": 601 }),
            json!({ "max_attachments_mb": 0 }),
            json!({ "max_attachments_mb": 4097 }),
        ] {
            assert!(check_ranges(&edit(body.clone())).is_err(), "{body}");
        }
        for body in [
            json!({ "guard_timeout_secs": 600 }),
            json!({ "max_attachments_mb": 4096 }),
            json!({ "session_cap": 1 }),
        ] {
            assert!(check_ranges(&edit(body.clone())).is_ok(), "{body}");
        }
        assert_eq!(
            check_ranges(&edit(json!({ "guard_timeout_secs": 0 }))).unwrap_err(),
            "guard_timeout_secs must be between 1 and 600 seconds"
        );
    }

    #[test]
    fn an_unknown_pin_lists_the_known_profiles() {
        let msg = unknown_manager_profile("Ghost", &["Default".into(), "Reviewer".into()]);
        assert!(msg.contains("`Ghost`") && msg.ends_with("Known profiles: Default, Reviewer"));
    }
}
