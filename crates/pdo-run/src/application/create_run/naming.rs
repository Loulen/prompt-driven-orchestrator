//! How a new Run is named and whether its input is acceptable.

use crate::application::node::prompt::RunNameHint;

/// Validate the user input against the pipeline's `prompt_required` flag. A
/// prompt-optional pipeline accepts empty input — its entry node sources its own
/// work — and treats a provided prompt as additional info.
pub fn validate_run_input(prompt_required: bool, input: &str) -> Result<(), String> {
    if prompt_required && input.trim().is_empty() {
        return Err("this pipeline requires a prompt: input must not be empty".to_string());
    }
    Ok(())
}

/// Deterministic placeholder display name for a run with neither a user-supplied
/// name nor any input to summarise. Derived from the run-id's own timestamp prefix
/// rather than a fresh clock read, so it matches the run-id. run-ids are always
/// `YYYYMMDD-HHMMSS-xxxxxxx` (≥ 23 ASCII chars), so `[..15]` cannot panic on a char
/// boundary.
pub fn placeholder_run_name(run_id: &str) -> String {
    format!("Untitled run {}", &run_id[..15])
}

/// Decide how the daemon should treat naming for a run at spawn.
///
/// `auto_name == false` ⇒ the manager is never instructed to rename. The `true`
/// branch is gated on `input`, NOT on the pipeline's `prompt_required` flag: an
/// empty input means there is nothing to summarise yet, so the daemon sets a
/// deterministic placeholder and the manager renames best-effort later.
///
/// This hint only governs whether the manager is *told to rename* — the display name
/// itself is chosen by `Runs::start_run`. Hence the `false` branch ignoring `name`:
/// a blank name still yields `UserProvided`, a stable placeholder with no rename
/// instruction, which is what the Trigger case needs.
pub fn run_name_hint(auto_name: bool, _name: Option<&str>, input: &str) -> RunNameHint {
    if !auto_name {
        RunNameHint::UserProvided
    } else if input.trim().is_empty() {
        RunNameHint::Placeholder
    } else {
        RunNameHint::DeriveFromInput
    }
}
