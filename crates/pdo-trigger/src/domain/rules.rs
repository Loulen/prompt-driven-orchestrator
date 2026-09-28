//! What a Trigger's configuration must satisfy, checked at every write (create
//! and edit) so the error lands where the operator can fix it — never at 3am
//! (ADR-0001).

/// A Trigger always has a name.
pub const EMPTY_NAME: &str = "trigger name must not be empty";

/// A pipeline that requires a prompt needs an input source.
pub const PROMPT_REQUIRED: &str = "this pipeline requires a prompt; add a guard, an input \
                                   template, or mark the pipeline prompt-not-required";

/// Whether `name` is a usable Trigger name.
pub fn valid_name(name: &str) -> bool {
    !name.trim().is_empty()
}

/// Validate a `max_concurrent` cap: when present it must be >= 1. Shared by create
/// and edit so the rule can't drift between them. A cap set while
/// `overlap_policy == "skip"` is accepted and stays inert.
pub fn validate_max_concurrent(v: Option<i64>) -> Result<(), &'static str> {
    match v {
        Some(n) if n < 1 => Err("max_concurrent must be >= 1"),
        _ => Ok(()),
    }
}

/// A Trigger with no guard has its input template as its only input source:
/// empty + prompt-required is a misconfiguration.
pub fn lacks_input(
    prompt_required: bool,
    guard_command: Option<&str>,
    input_template: &str,
) -> bool {
    prompt_required
        && guard_command.unwrap_or("").trim().is_empty()
        && input_template.trim().is_empty()
}

/// The two overlap policies a Trigger stores: `allow`, or `skip` for anything
/// else.
pub fn overlap_policy(raw: &str) -> String {
    if raw == "allow" {
        "allow".to_string()
    } else {
        "skip".to_string()
    }
}

/// A blank value is no value: it inherits instead of naming `""`.
pub fn non_blank(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cap_below_one_is_refused() {
        assert!(validate_max_concurrent(Some(0)).is_err());
        assert!(validate_max_concurrent(Some(-3)).is_err());
        assert!(validate_max_concurrent(Some(1)).is_ok());
        assert!(validate_max_concurrent(None).is_ok());
    }

    #[test]
    fn a_prompt_required_pipeline_needs_a_guard_or_a_template() {
        assert!(lacks_input(true, None, "  "));
        assert!(lacks_input(true, Some(" "), ""));
        assert!(!lacks_input(true, Some("echo hi"), ""));
        assert!(!lacks_input(true, None, "do it"));
        assert!(!lacks_input(false, None, ""));
    }

    #[test]
    fn anything_but_allow_is_skip() {
        assert_eq!(overlap_policy("allow"), "allow");
        assert_eq!(overlap_policy("skip"), "skip");
        assert_eq!(overlap_policy("bogus"), "skip");
    }

    #[test]
    fn names_and_blanks() {
        assert!(!valid_name("  "));
        assert!(valid_name("weekly"));
        assert_eq!(non_blank(Some("  ".into())), None);
        assert_eq!(non_blank(Some("x".into())), Some("x".into()));
    }
}
