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
//! (`[a|b|c]`, `<a|b|c>`, `Choices: a, b, c`, `One of: …`, a bare parenthesised
//! `(a, b, c)`, a run of quoted ids `'a', 'b', 'c'`). It can go **blind** to a
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

/// Hard cap on how far past a `--model` / `--effort` flag token we scan. The real
/// bound is [`option_block`] — the next option line — so this only keeps a help text
/// with no option structure at all (one long paragraph) from being scanned whole.
/// Generous on purpose: `claude` spells its `--model` offer over five wrapped lines.
const WINDOW: usize = 800;

/// Left margin, in columns, at or below which a line starting with `-` opens a **new
/// option** rather than continuing the current one. Every CLI in play prints its
/// options at column 2..=6 and wraps their descriptions far to the right (column 40),
/// so this separates the two without parsing the layout.
const OPTION_INDENT: usize = 8;

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
            let choices = extract_enum(option_block(help, after));
            if !choices.is_empty() {
                return choices;
            }
        }
    }
    Vec::new()
}

/// The scan window for a flag found at byte `from`: **its own option block** —
/// everything from just past the flag token to the start of the next option line —
/// capped at [`WINDOW`].
///
/// The cap alone is not a bound: `claude` and `copilot` wrap descriptions over four
/// or five 80-column lines, so 400 characters routinely spill into the *next* two
/// options. That is how `copilot`'s `--model` (which enumerates nothing) would
/// harvest `--mouse`'s `(on|off)` and offer "on" and "off" as models. Ending the
/// window at the next option line keeps every reader inside the blurb that belongs
/// to the flag, which is the only place an enumeration for it can legitimately sit.
fn option_block(help: &str, from: usize) -> &str {
    let end = floor_char_boundary(help, (from + WINDOW).min(help.len()));
    let capped = &help[from..end];
    let mut offset = 0;
    for line in capped.split_inclusive('\n') {
        // The first line is the flag's own — its leading text is the flag itself,
        // so the "new option" test would always fire on it.
        if offset > 0 && opens_a_new_option(line) {
            return &capped[..offset];
        }
        offset += line.len();
    }
    capped
}

/// Whether `line` opens a new option: a `-` at or left of [`OPTION_INDENT`]. A
/// wrapped description line either does not start with `-` or sits far to the right.
fn opens_a_new_option(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && line.len() - trimmed.len() <= OPTION_INDENT
}

/// The largest char boundary `<= idx`. `--help` is prose and may carry an em dash;
/// slicing a byte offset straight into it would panic in the daemon's probe path.
fn floor_char_boundary(s: &str, mut idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    while !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
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
            || matches!(
                bytes[idx - 1],
                b' ' | b'\t' | b'\n' | b',' | b'(' | b'|' | b'/'
            );
        let after_ok = end >= help.len()
            || matches!(bytes[end], b' ' | b'\t' | b'=' | b'<' | b'[' | b'\n' | b',');
        if before_ok && after_ok {
            return Some(end);
        }
        from = end;
    }
    None
}

/// Read the first enumeration in `window`, whichever convention spells it: a
/// bracketed pipe list (`[a|b|c]` / `<a|b|c>`), a keyword list (`Choices: a, b, c`),
/// a bare parenthesised list (`(low, medium, high)`), or a run of quoted ids
/// (`'fable', 'opus', 'sonnet'`). The **earliest** match wins, so a `<model>`
/// placeholder immediately followed by `Choices: …` still resolves, and a prose
/// parenthetical that precedes the real list never displaces it.
fn extract_enum(window: &str) -> Vec<String> {
    [
        bracketed_pipe_list(window),
        keyword_list(window),
        parenthesised_list(window),
        quoted_id_run(window),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|(idx, _)| *idx)
    .map(|(_, tokens)| tokens)
    .unwrap_or_default()
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
/// `window`, comma- or pipe-separated. Returns the keyword's start offset and the
/// parsed tokens.
///
/// The words a CLI puts in front of an enumeration. Shared with
/// [`parenthesised_list`], which stands aside whenever one of these opens the group
/// — the keyword reader is the one that knows how to bound a wrapped list.
const KEYS: &[&str] = &["choices:", "one of:", "values:", "allowed:", "supported:"];

/// A flat list ends at its line (`Choices: a, b, c`). But a CLI may **wrap** a long
/// enumeration across continuation lines *inside a parenthesis* — copilot 1.0.80
/// prints `… (choices:\n "none", "minimal",\n … "max")`. When the keyword sits inside
/// an unclosed `(`, the list is read to the matching `)` instead, so the wrapped
/// values are not truncated at the first newline. Stopping at the line there would
/// yield an empty axis and silently drop copilot's seven effort stops (#616 FP).
fn keyword_list(window: &str) -> Option<(usize, Vec<String>)> {
    let lower = window.to_ascii_lowercase();
    let mut best: Option<(usize, Vec<String>)> = None;
    for key in KEYS {
        if let Some(idx) = lower.find(key) {
            let start = idx + key.len();
            // Is the keyword inside an unclosed parenthesis? Then the enumeration may
            // wrap across lines; read to the closing `)`. Otherwise it is a flat list
            // that ends at its line.
            let before = &window[..idx];
            let inside_paren = before.matches('(').count() > before.matches(')').count();
            let bound = if inside_paren { ')' } else { '\n' };
            let seg_end = window[start..]
                .find(bound)
                .map(|r| start + r)
                .unwrap_or(window.len());
            let segment = &window[start..seg_end];
            let tokens = split_tokens(segment, &[',', '|']);
            if tokens.len() >= 2 && best.as_ref().is_none_or(|(bi, _)| idx < *bi) {
                best = Some((idx, tokens));
            }
        }
    }
    best
}

/// The first `(…)` group in `window` whose inner text is a comma- or pipe-separated
/// list of ≥2 plausible tokens, with **no** introducing keyword — `claude`'s form:
///
/// ```text
///   --effort <level>   Effort level for the current session
///                      (low, medium, high, xhigh, max)
/// ```
///
/// Prose parentheticals are not a hazard: the plausibility filter drops anything
/// carrying a space, so `(can be used multiple times)` and `(only works with
/// --print)` yield nothing and the scan moves on to the next group. Groups are
/// tried in order, so a `(e.g. …)` aside before the real list does not end the hunt.
///
/// A group opened by a [`KEYS`] word (`(choices: "none", "minimal", …)`) is **left
/// to** [`keyword_list`]: reading it here would splice the keyword onto the first
/// value and silently drop it — which is how copilot's `none` stop went missing.
fn parenthesised_list(window: &str) -> Option<(usize, Vec<String>)> {
    let mut from = 0;
    while let Some(rel_open) = window[from..].find('(') {
        let open = from + rel_open;
        // An unclosed `(` ends the hunt: there is no group to read.
        let close = open + 1 + window[open + 1..].find(')')?;
        let inner = &window[open + 1..close];
        let introduced = {
            let lower = inner.to_ascii_lowercase();
            KEYS.iter().any(|k| lower.contains(k))
        };
        if !introduced {
            let tokens = split_tokens(inner, &[',', '|']);
            if tokens.len() >= 2 {
                return Some((open, tokens));
            }
        }
        from = close + 1;
    }
    None
}

/// A run of ≥2 distinct quoted ids in `window` — `claude`'s `--model` form, where
/// the offer is spelled as prose around quoted aliases:
///
/// ```text
///   --model <model>   Model for the current session. Provide an alias for the
///                     latest model (e.g. 'fable', 'opus', or 'sonnet') or a
///                     model's full name (e.g. 'claude-fable-5').
/// ```
///
/// Reading the prose as a list is what the two structured readers cannot do here:
/// the commas sit between quoted ids *and* the word "or", so splitting the
/// parenthetical yields one usable token, not three. Quoting is the signal — the
/// binary marks each id, and an unquoted word is never harvested. Returns the offset
/// of the first quoted id, so an enumeration spelled earlier in the block still wins.
/// Requiring two distinct ids keeps a lone `use 'auto' to …` (copilot 1.0.80) an
/// absence, which is what the picker must keep degrading on.
fn quoted_id_run(window: &str) -> Option<(usize, Vec<String>)> {
    let mut tokens: Vec<String> = Vec::new();
    let mut first: Option<usize> = None;
    let mut rest = window;
    let mut base = 0;
    while let Some(rel_open) = rest.find(['\'', '"']) {
        let quote = rest.as_bytes()[rel_open];
        let inner_start = rel_open + 1;
        let Some(rel_close) = rest[inner_start..].find(quote as char) else {
            break;
        };
        let close = inner_start + rel_close;
        let tok = &rest[inner_start..close];
        if is_plausible_value(tok) {
            if first.is_none() {
                first = Some(base + rel_open);
            }
            if !tokens.iter().any(|e| e == tok) {
                tokens.push(tok.to_string());
            }
        }
        base += close + 1;
        rest = &rest[close + 1..];
    }
    match (first, tokens.len()) {
        (Some(idx), n) if n >= 2 => Some((idx, tokens)),
        _ => None,
    }
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
        let help = "  --effort <level>   [default|min|low|medium|high|max|ultra]\n";
        let cat = parse_help(help);
        assert_eq!(
            cat.efforts,
            vec!["default", "min", "low", "medium", "high", "max", "ultra"]
        );
    }

    #[test]
    fn wrapped_parenthesised_choices_read_across_continuation_lines() {
        // copilot 1.0.80 verbatim: the effort keyword sits at end of line, inside an
        // unclosed `(`, and its values wrap onto continuation lines with quotes. The
        // reader must follow to the closing `)` — stopping at the first newline gave
        // an empty axis and dropped all seven stops (#616 FP, the motivating bug).
        let help = "\
  --effort, --reasoning-effort <level>  Set the reasoning effort level (choices:
                                        \"none\", \"minimal\", \"low\", \"medium\",
                                        \"high\", \"xhigh\", \"max\")
  --model <model>                       Set the AI model to use (use 'auto' to
                                        let Copilot pick automatically)
";
        let cat = parse_help(help);
        assert_eq!(
            cat.efforts,
            vec!["none", "minimal", "low", "medium", "high", "xhigh", "max"],
            "the seven wrapped effort stops must all be read"
        );
        assert!(cat.has_effort_axis());
        // `--model` enumerates nothing (just `use 'auto'`) ⇒ free-text fallback.
        assert!(
            cat.models.is_empty(),
            "no model enumeration ⇒ declared absence"
        );
    }

    #[test]
    fn a_flat_line_list_still_stops_at_its_line() {
        // The paren branch must not leak: a flat `Choices:` list ends at its line, so
        // a following flag's own list is never harvested onto this one.
        let help = "\
  --model <model>    Choices: sonnet, opus, haiku
  --effort <level>   One of: low, medium, high
";
        let cat = parse_help(help);
        assert_eq!(cat.models, vec!["sonnet", "opus", "haiku"]);
        assert_eq!(cat.efforts, vec!["low", "medium", "high"]);
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
        assert_eq!(
            cat.models,
            vec!["sonnet", "opus"],
            "matched the real --model"
        );
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
    fn claudes_two_forms_are_read_verbatim_from_its_help() {
        // #617 FP finding 2: `claude --help` prints neither a bracketed list nor a
        // `Choices:` keyword — its effort stops sit in a bare parenthesis on the
        // wrapped line, and its model aliases are quoted inside prose. Both axes came
        // back empty, so BOTH pickers degraded to free text on a `claude` node while
        // the binary was plainly enumerating. Verbatim from claude 2.x.
        let help = "\
  --debug                               Enable debug mode
  --effort <level>                      Effort level for the current session
                                        (low, medium, high, xhigh, max)
  --environment <environment_id>        Create a new cloud session that runs on
                                        the given self-hosted environment
  --model <model>                       Model for the current session. Provide
                                        an alias for the latest model (e.g.
                                        'fable', 'opus', or 'sonnet') or a
                                        model's full name (e.g.
                                        'claude-fable-5').
  -n, --name <name>                     Set a display name for this session
                                        (shown in the prompt box, /resume
                                        picker, and terminal title)
";
        let cat = parse_help(help);
        assert_eq!(
            cat.efforts,
            vec!["low", "medium", "high", "xhigh", "max"],
            "the parenthesised effort stops must be read"
        );
        assert!(cat.has_effort_axis());
        assert_eq!(
            cat.models,
            vec!["fable", "opus", "sonnet"],
            "the quoted aliases, and nothing the prose merely mentions"
        );
    }

    #[test]
    fn a_neighbouring_options_list_is_not_harvested_onto_a_flag_that_enumerates_none() {
        // The regression the option-block bound exists to kill: copilot's `--model`
        // enumerates nothing, and two lines below it `--mouse` prints `(on|off)`.
        // Scanning a flat character window would offer "on" and "off" as MODELS —
        // worse than the empty catalogue, because it looks like an answer. Verbatim
        // from copilot 1.0.80.
        let help = "\
  --model <model>                       Set the AI model to use (use 'auto' to
                                        let Copilot pick automatically)
  --mouse[=value]                       Enable mouse support in alt screen mode
                                        (on|off)
  -n, --name <name>                     Set a name for the new session
";
        let cat = parse_help(help);
        assert!(
            cat.models.is_empty(),
            "copilot declares no model offer; the neighbour's (on|off) is not one"
        );
    }

    #[test]
    fn a_bare_parenthesised_list_is_an_enumeration_prose_is_not() {
        // The parenthesised reader must fire on a list and stay silent on prose —
        // the plausibility filter is what separates them, so `(can be used multiple
        // times)` and `(only works with --print)` yield nothing.
        let listed = "  --effort <level>   Reasoning effort (none, low, high)\n";
        assert_eq!(parse_help(listed).efforts, vec!["none", "low", "high"]);

        let prose = "\
  --model <model>    The model to use (can be used multiple times, only works
                     with --print)
";
        assert!(
            parse_help(prose).models.is_empty(),
            "a prose parenthetical is not an offer"
        );
    }

    #[test]
    fn a_single_quoted_id_stays_an_absence() {
        // copilot 1.0.80's `--model` quotes exactly one id (`'auto'`), which is a
        // sentinel, not a catalogue. One quoted id ⇒ free-text field (#629's ticket
        // stands): two is the threshold that makes a run a list.
        let help = "  --model <model>   Set the AI model to use (use 'auto' to let it pick)\n";
        assert!(parse_help(help).models.is_empty());
    }

    #[test]
    fn a_prose_apostrophe_never_becomes_a_model_id() {
        // `model's` puts a stray quote between the real ids. The span it pairs with
        // carries spaces, so the plausibility filter drops it — the ids around it
        // survive untouched.
        let help = "  --model <m>   Provide 'opus' or 'sonnet', or the model's full name\n";
        assert_eq!(parse_help(help).models, vec!["opus", "sonnet"]);
    }

    #[test]
    fn an_em_dash_in_the_blurb_does_not_panic_the_scan() {
        // `--help` is prose and carries non-ASCII (copilot's header has an em dash).
        // Slicing a raw byte offset into it is a panic waiting on a description long
        // enough to reach the cap — in the daemon's boot probe.
        let mut prefix = String::from("  --model <m>   Choices: opus, sonnet\n");
        // Byte offset just past the `--model` token — where the scan window opens.
        let from = "  --model".len();
        // Pad so the cap lands on an em dash's *second* byte, which is no boundary.
        while (from + WINDOW - prefix.len()) % 3 != 1 {
            prefix.push(' ');
        }
        let help = format!("{prefix}{}", "—".repeat(400));
        assert_eq!(parse_help(&help).models, vec!["opus", "sonnet"]);
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
