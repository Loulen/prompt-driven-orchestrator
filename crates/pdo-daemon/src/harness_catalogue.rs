//! Deduce a harness's **offered** model & effort catalogue from its installed
//! binary (#616, ADR-0053).
//!
//! ADR-0001 keeps the **value** free-text pass-through — a node launches with
//! whatever model/effort string the YAML carries, verbatim, no closed enum. This
//! module is the other half of ADR-0053: what the interface **offers** ceases to
//! be a client-side hard-coded list and becomes a property of *what is installed
//! on the machine*. The daemon reads the offer from the binary; the client renders
//! it. We deduce what we **offer**, we never validate what we **receive**.
//!
//! ## Pure parser, impure runner
//!
//! This module is **pure**: raw help text in, a [`Catalogue`] out — no `$PATH`, no
//! subprocess, no clock (the discipline `run_cost`/`harness_registry` keep). The
//! act of *running* the binary (and reading its version) lives beside
//! [`crate::tmux_session_manager::binary_available`], the one place that reads the
//! environment. The two responsibilities meet in [`crate::lib`]'s settings view and
//! the boot probe.
//!
//! ## Best-effort, never a contract (ADR-0053 §Limites)
//!
//! A binary's `--help` is generated prose, not an API. The parser scans it for the
//! enumerations a CLI conventionally prints beside `--model` / `--effort`
//! (`[a|b|c]`, `<a|b|c>`, `Choices: a, b, c`, `One of: …`). It can go **blind** to a
//! release that reworks its help — and that is fine: an empty catalogue degrades to
//! the free-text field, the path that cannot break. The catalogue is a **commodity**
//! (a convenience for the picker), the free-text escape hatch is the guarantee.
//!
//! A harness whose binary prints no enumeration (measured: `opencode` takes a bare
//! `provider/model` with no list) yields an empty catalogue — a **declared absence**,
//! rendered as the free-text field, exactly like a missing effort axis.

/// The offered catalogue for one harness: the model ids and effort levels its
/// installed binary enumerates, in first-seen order, de-duplicated. Empty on either
/// axis ⇒ that axis has no offer and the client falls back to free text (an absence
/// declared by the binary, not a default).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Catalogue {
    /// Offered model ids (`--model` enumeration), verbatim as the binary spells
    /// them. A value the picker does not carry is still accepted (ADR-0001): this
    /// is an offer, never a guard.
    pub(crate) models: Vec<String>,
    /// Offered effort levels (`--effort` enumeration). Empty ⇒ this binary exposes
    /// no effort axis; the client greys the picker.
    pub(crate) efforts: Vec<String>,
}

impl Catalogue {
    /// Whether the binary enumerates an effort axis — the **served** fact the
    /// client's effort-picker greying now reads instead of a hard-coded map
    /// (ADR-0053, closes the client-side `HARNESS_HAS_EFFORT`). A harness whose
    /// launch template carries an `{effort}` hole but whose help enumerates none
    /// (or vice-versa) is folded in by the caller — see the settings view.
    pub(crate) fn has_effort_axis(&self) -> bool {
        !self.efforts.is_empty()
    }
}

/// A catalogue cached against the binary **version** it was read from (#616,
/// ADR-0053 §3). The version is the freshness key: on the next read, if the binary
/// reports a different version, the cached catalogue is stale and a fresh probe
/// runs — so an auto-updating binary is followed without a daemon restart. `version`
/// is `None` when the binary answered no `--version`; a later `Some`/different value
/// still invalidates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct CachedCatalogue {
    pub(crate) version: Option<String>,
    pub(crate) catalogue: Catalogue,
}

/// How far past a `--model` / `--effort` flag token we scan for its enumeration —
/// one option's blurb, generously. Bounded so a later, unrelated option's list is
/// never harvested onto the wrong flag.
const WINDOW: usize = 400;

/// Parse a binary's `--help` text into its offered catalogue. PURE and
/// harness-agnostic: every CLI in play spells the flags `--model` / `--effort`, and
/// the enumeration conventions are shared, so the same reader serves `claude`,
/// `copilot`, and any disk-declared harness. A harness that prints no enumeration
/// (opencode) yields [`Catalogue::default`] — the free-text fallback.
pub(crate) fn parse_help(help: &str) -> Catalogue {
    Catalogue {
        models: extract_choices(help, &["--model", "-m"]),
        efforts: extract_choices(help, &["--effort", "--reasoning-effort"]),
    }
}

/// Extract the enumerated choices printed beside the first of `flags` that appears
/// in `help`. Returns an empty vec when no flag matches or the matched flag carries
/// no enumeration (a bare `<model>` placeholder, e.g. opencode's free-text model).
fn extract_choices(help: &str, flags: &[&str]) -> Vec<String> {
    for flag in flags {
        if let Some(after) = find_flag_token(help, flag) {
            let window = &help[after..(after + WINDOW).min(help.len())];
            let choices = extract_enum(window);
            if !choices.is_empty() {
                return choices;
            }
        }
    }
    Vec::new()
}

/// Find `flag` as a **whole token** in `help` and return the byte index just past
/// it. "Whole token" means it is not the tail of a longer word: it starts at the
/// string head or after a separator, and ends at a separator or an argument opener
/// (`=`, `<`, `[`). This keeps `--model` from matching inside `--model-family` and
/// lands the scan window exactly on the flag's argument/description.
fn find_flag_token(help: &str, flag: &str) -> Option<usize> {
    let bytes = help.as_bytes();
    let mut from = 0;
    while let Some(rel) = help[from..].find(flag) {
        let idx = from + rel;
        let end = idx + flag.len();
        let before_ok = idx == 0
            || matches!(bytes[idx - 1], b' ' | b'\t' | b'\n' | b',' | b'(' | b'|' | b'/');
        let after_ok = end >= help.len()
            || matches!(bytes[end], b' ' | b'\t' | b'=' | b'<' | b'[' | b'\n' | b',');
        if before_ok && after_ok {
            return Some(end);
        }
        from = end;
    }
    None
}

/// Read the first enumeration in `window`: a bracketed pipe list (`[a|b|c]` /
/// `<a|b|c>`) or a keyword list (`Choices: a, b, c`). Whichever appears first wins,
/// so a `<model>` placeholder immediately followed by `Choices: …` still resolves.
fn extract_enum(window: &str) -> Vec<String> {
    let bracket = bracketed_pipe_list(window);
    let keyword = keyword_list(window);
    match (bracket, keyword) {
        (Some((bi, bv)), Some((ki, kv))) => {
            if bi <= ki {
                bv
            } else {
                kv
            }
        }
        (Some((_, bv)), None) => bv,
        (None, Some((_, kv))) => kv,
        (None, None) => Vec::new(),
    }
}

/// The first `[…]`/`<…>` group in `window` whose inner text is a pipe-separated
/// list of ≥2 plausible tokens. Returns its start offset (for ordering against a
/// keyword list) and the parsed tokens. A single-token group (`<model>`) is skipped
/// — a placeholder, not an enumeration.
fn bracketed_pipe_list(window: &str) -> Option<(usize, Vec<String>)> {
    let bytes = window.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let (open, close) = match bytes[i] {
            b'[' => (b'[', b']'),
            b'<' => (b'<', b'>'),
            _ => {
                i += 1;
                continue;
            }
        };
        let _ = open;
        if let Some(rel_close) = window[i + 1..].find(close as char) {
            let inner = &window[i + 1..i + 1 + rel_close];
            if inner.contains('|') {
                let tokens = split_tokens(inner, &['|']);
                if tokens.len() >= 2 {
                    return Some((i, tokens));
                }
            }
            i = i + 1 + rel_close + 1;
        } else {
            break;
        }
    }
    None
}

/// The first `Choices:` / `One of:` / `Values:` / `Allowed:` / `Supported:` list in
/// `window`, comma- or pipe-separated, read to end of line. Returns the keyword's
/// start offset and the parsed tokens.
fn keyword_list(window: &str) -> Option<(usize, Vec<String>)> {
    const KEYS: &[&str] = &["choices:", "one of:", "values:", "allowed:", "supported:"];
    let lower = window.to_ascii_lowercase();
    let mut best: Option<(usize, Vec<String>)> = None;
    for key in KEYS {
        if let Some(idx) = lower.find(key) {
            let start = idx + key.len();
            let line_end = window[start..]
                .find('\n')
                .map(|r| start + r)
                .unwrap_or(window.len());
            let segment = &window[start..line_end];
            let tokens = split_tokens(segment, &[',', '|']);
            if tokens.len() >= 2 && best.as_ref().is_none_or(|(bi, _)| idx < *bi) {
                best = Some((idx, tokens));
            }
        }
    }
    best
}

/// Split `s` on any of `seps`, keep only plausible flag values, de-duplicate
/// preserving order. A "plausible value" is a short run of the characters CLI
/// model/effort ids use (`gpt-5-codex`, `claude-sonnet-4.5`, `openrouter/foo`,
/// `xhigh`) — this drops prose words that sneak past a mis-parsed list.
fn split_tokens(s: &str, seps: &[char]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in s.split(|c| seps.contains(&c)) {
        let tok = raw
            .trim()
            .trim_matches(|c: char| c == '.' || c == '"' || c == '\'' || c == '`');
        if is_plausible_value(tok) && !out.iter().any(|e| e == tok) {
            out.push(tok.to_string());
        }
    }
    out
}

/// Whether `tok` looks like a model/effort id rather than prose: 1..=60 chars, made
/// only of alphanumerics and the id punctuation `-._/:`. Rejects the empty string,
/// anything with whitespace, and anything carrying prose punctuation.
fn is_plausible_value(tok: &str) -> bool {
    !tok.is_empty()
        && tok.len() <= 60
        && tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/' | ':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_bracketed_pipe_model_list() {
        // copilot-shape help: models in a `[a|b|c]` group beside `--model`.
        let help = "\
Usage: copilot [options]

Options:
  --model <model>   The AI model to use [gpt-5|gpt-5-codex|gpt-5-mini|claude-sonnet-4.5|o4-mini|gemini-2.5-pro]
  --allow-all       Grant full autonomy
";
        let cat = parse_help(help);
        assert_eq!(
            cat.models,
            vec![
                "gpt-5",
                "gpt-5-codex",
                "gpt-5-mini",
                "claude-sonnet-4.5",
                "o4-mini",
                "gemini-2.5-pro"
            ]
        );
        // No effort enumeration in this help ⇒ no effort axis.
        assert!(cat.efforts.is_empty());
        assert!(!cat.has_effort_axis());
    }

    #[test]
    fn parses_a_keyword_effort_list_and_a_keyword_model_list() {
        // claude-shape help: `Choices:` / `One of:` comma lists.
        let help = "\
  --model <model>    Model for the session. Choices: sonnet, opus, haiku, opusplan
  --effort <level>   Reasoning effort. One of: low, medium, high, max
";
        let cat = parse_help(help);
        assert_eq!(cat.models, vec!["sonnet", "opus", "haiku", "opusplan"]);
        assert_eq!(cat.efforts, vec!["low", "medium", "high", "max"]);
        assert!(cat.has_effort_axis());
    }

    #[test]
    fn a_bare_placeholder_is_not_an_enumeration() {
        // opencode-shape: `--model <provider/model>` is a placeholder, not a list —
        // the free-text fallback (a declared absence, ADR-0053 §Limites).
        let help = "  --model <provider/model>   Model to use in the format provider/model\n";
        let cat = parse_help(help);
        assert!(cat.models.is_empty(), "a single placeholder is no offer");
        assert!(cat.efforts.is_empty());
    }

    #[test]
    fn seven_effort_stops_including_ones_claude_has_no_name_for() {
        // AC #4: a harness may enumerate more effort stops than claude — the picker
        // renders whatever the binary offers, not a curated five.
        let help =
            "  --effort <level>   [default|min|low|medium|high|max|ultra]\n";
        let cat = parse_help(help);
        assert_eq!(
            cat.efforts,
            vec!["default", "min", "low", "medium", "high", "max", "ultra"]
        );
    }

    #[test]
    fn flag_token_is_matched_whole_not_as_a_prefix() {
        // `--model` must not match inside `--model-family`, which would land the
        // window on the wrong option and harvest the wrong list.
        let help = "\
  --model-family <fam>   [anthropic|openai]
  --model <model>        Choices: sonnet, opus
";
        let cat = parse_help(help);
        assert_eq!(cat.models, vec!["sonnet", "opus"], "matched the real --model");
    }

    #[test]
    fn empty_help_is_an_empty_catalogue() {
        assert_eq!(parse_help(""), Catalogue::default());
        assert_eq!(parse_help("no flags here at all"), Catalogue::default());
    }

    #[test]
    fn duplicate_ids_are_collapsed_preserving_order() {
        let help = "  --model <m>  [opus|sonnet|opus|haiku]\n";
        assert_eq!(parse_help(help).models, vec!["opus", "sonnet", "haiku"]);
    }

    #[test]
    fn prose_is_rejected_as_a_model_id() {
        // A mis-formatted help whose "list" is actually a sentence must not turn
        // prose into model ids — the plausibility filter drops multi-word tokens.
        let help = "  --model <m>   Choices: the default model, or something else\n";
        // "the default model" and "or something else" carry spaces ⇒ rejected whole.
        assert!(parse_help(help).models.is_empty());
    }

    #[test]
    fn slashed_and_dotted_ids_survive_the_filter() {
        let help = "  --model <m>   [openrouter/foo|anthropic/claude-3.5|x_y]\n";
        assert_eq!(
            parse_help(help).models,
            vec!["openrouter/foo", "anthropic/claude-3.5", "x_y"]
        );
    }
}
