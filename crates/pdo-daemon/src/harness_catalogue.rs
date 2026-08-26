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
//! ## Three sources, machine-generated first (#629, ADR-0056)
//!
//! A binary does not always print its enumeration where a `--help` reader looks.
//! Measured on `copilot` 1.0.80: `--help` enumerates the effort stops but describes
//! `--model` in prose, while the ids live in two other places. So the module exposes
//! **three readers**, which the runner tries in preference order:
//!
//! 1. [`parse_completion_script`] — the shell-completion script (`<bin> completion
//!    bash`). Machine-**generated** from the CLI's own declared choices, so it is the
//!    most stable of the three and the one ADR-0056 prefers.
//! 2. [`parse_settings_prose`] — the settings help topic (`<bin> help config`), where
//!    a CLI documents each setting and bullets its allowed values.
//! 3. [`parse_help`] — the `--help` reader of #616.
//!
//! Each axis (models, efforts) takes the **highest-preference source that offers one**
//! — see [`Catalogue::fill_missing_from`]. The two richer sources are only *run*
//! against a binary whose `--help` declares them ([`advertises_subcommand`]); the
//! runner's doc says why that gate is load-bearing.
//!
//! ## Best-effort, never a contract (ADR-0053 §Limites)
//!
//! None of the three is an API: `--help` and `help config` are prose, a completion
//! script is generated bash. Every reader is best-effort and can go **blind** to a
//! release that reworks its output — and that is fine: an empty catalogue degrades to
//! the free-text field, the path that cannot break. The catalogue is a **commodity**
//! (a convenience for the picker), the free-text escape hatch is the guarantee.
//!
//! A harness whose binary prints no enumeration anywhere (measured: `opencode` takes
//! a bare `provider/model` with no list) yields an empty catalogue — a **declared
//! absence**, rendered as the free-text field, exactly like a missing effort axis.

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

    /// Fold a lower-preference source into this one, **per axis** (#629, ADR-0056).
    /// An axis this catalogue already offers is kept; an axis it lacks is taken from
    /// `other`. Axis-wise and not whole-catalogue, because the sources disagree about
    /// what they cover: copilot's completion script carries both axes, its
    /// `help config` only models, its `--help` only efforts.
    ///
    /// The caller folds in **preference order**, so "already offered" means "answered
    /// by a source that outranks this one" — not "answered by whichever ran first".
    pub(crate) fn fill_missing_from(&mut self, other: Catalogue) {
        if self.models.is_empty() {
            self.models = other.models;
        }
        if self.efforts.is_empty() {
            self.efforts = other.efforts;
        }
    }

    /// Whether both axes are filled — the runner's short-circuit: with models *and*
    /// efforts already answered, a lower-preference source has nothing left to
    /// contribute and its subprocess is not spent.
    pub(crate) fn is_complete(&self) -> bool {
        !self.models.is_empty() && !self.efforts.is_empty()
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
            let choices = extract_enum(window_from(help, after, WINDOW));
            if !choices.is_empty() {
                return choices;
            }
        }
    }
    Vec::new()
}

/// The `len`-byte window of `text` starting at `from`, **snapped down to a char
/// boundary** at both ends. Help text is not ASCII (copilot's prints an em dash), so
/// a naive `&text[a..a + len]` can slice mid-codepoint and panic — inside the
/// `/settings` handler, on a machine whose only sin is having that binary installed.
/// Truncating the window one character early is the harmless failure here.
fn window_from(text: &str, from: usize, len: usize) -> &str {
    let start = floor_boundary(text, from);
    let end = floor_boundary(text, start.saturating_add(len).min(text.len()));
    &text[start..end]
}

/// The largest byte index `<= i` that is a char boundary of `text` (`i` clamped into
/// range first).
fn floor_boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Find `flag` as a **whole token** in `help` and return the byte index just past
/// its first occurrence. See [`flag_token_positions`].
fn find_flag_token(help: &str, flag: &str) -> Option<usize> {
    flag_token_positions(help, flag).into_iter().next()
}

/// Every byte index just past a **whole-token** occurrence of `flag` in `text`, in
/// order. "Whole token" means it is not the tail of a longer word: it starts at the
/// string head or after a separator, and ends at a separator, an argument opener
/// (`=`, `<`, `[`) or a shell-`case` pattern terminator (`)`, `|`). This keeps
/// `--model` from matching inside `--model-family` and lands the scan window exactly
/// on the flag's argument/description — or, in a completion script, on its case arm.
///
/// All positions, not just the first: a completion script names every flag twice
/// over (once in a flat "all flags" word list, once as a case arm), and only the
/// case arm carries the choices.
fn flag_token_positions(text: &str, flag: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = text[from..].find(flag) {
        let idx = from + rel;
        let end = idx + flag.len();
        let before_ok = idx == 0
            || matches!(
                bytes[idx - 1],
                b' ' | b'\t' | b'\n' | b',' | b'(' | b'|' | b'/'
            );
        let after_ok = end >= text.len()
            || matches!(
                bytes[end],
                b' ' | b'\t' | b'=' | b'<' | b'[' | b'\n' | b',' | b')' | b'|'
            );
        if before_ok && after_ok {
            out.push(end);
        }
        from = end;
    }
    out
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
/// `window`, comma- or pipe-separated. Returns the keyword's start offset and the
/// parsed tokens.
///
/// A flat list ends at its line (`Choices: a, b, c`). But a CLI may **wrap** a long
/// enumeration across continuation lines *inside a parenthesis* — copilot 1.0.80
/// prints `… (choices:\n "none", "minimal",\n … "max")`. When the keyword sits inside
/// an unclosed `(`, the list is read to the matching `)` instead, so the wrapped
/// values are not truncated at the first newline. Stopping at the line there would
/// yield an empty axis and silently drop copilot's seven effort stops (#616 FP).
fn keyword_list(window: &str) -> Option<(usize, Vec<String>)> {
    const KEYS: &[&str] = &["choices:", "one of:", "values:", "allowed:", "supported:"];
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

/// Whether `help` (a binary's `--help`) declares a subcommand called `name` (#629,
/// ADR-0056). PURE; the runner uses it to decide which of the richer sources it may
/// run at all.
///
/// This gate is load-bearing, not cosmetic. A CLI that has no such subcommand does not
/// necessarily *refuse* it: measured, `claude completion bash` is read as a **prompt**
/// and opens a session that idles until the probe timeout kills it. Running a
/// subcommand a binary never advertised is how a catalogue probe turns into a
/// five-second stall inside a `/settings` response.
///
/// A command list is one command per line, either bare (`  completion <shell>`) or
/// prefixed with the binary name (`  opencode completion   generate …`) — so the test
/// is "the line's first or second word is exactly `name`". Deliberately loose: a false
/// positive costs one bounded probe that finds nothing, a false negative costs the
/// catalogue.
pub(crate) fn advertises_subcommand(help: &str, name: &str) -> bool {
    help.lines().any(|line| {
        let mut words = line.split_whitespace();
        let (first, second) = (words.next(), words.next());
        first == Some(name) || second == Some(name)
    })
}

// ---------------------------------------------------------------------------
// Source 1 — the generated shell-completion script (#629, ADR-0056)
// ---------------------------------------------------------------------------

/// Backstop bound on one case arm's body, for a generator that ends its arms some
/// way other than `;;` / `esac`. The `compgen -W` list *itself* may be far longer
/// (copilot's is ~700 chars); this bounds only where we look for the `-W`.
const COMPGEN_WINDOW: usize = 200;

/// Parse a bash **completion script** (`<bin> completion bash`) into its offered
/// catalogue (#629, ADR-0056). Machine-generated from the CLI's own declared choices
/// — the preferred source, because unlike help prose it exists to be read by a
/// program.
///
/// The shape every generator emits is a `case` on the previous word:
///
/// ```text
///     --model)
///         COMPREPLY=( $(compgen -W 'auto gpt-5.5 claude-opus-5' -- "$cur") )
///         ;;
///     --effort|--reasoning-effort)
///         COMPREPLY=( $(compgen -W 'none low high' -- "$cur") )
/// ```
///
/// A binary with no `completion` subcommand, or one whose script declares no choices
/// for these flags, yields [`Catalogue::default`] and the runner falls through to the
/// next source.
pub(crate) fn parse_completion_script(script: &str) -> Catalogue {
    Catalogue {
        models: completion_words(script, &["--model", "-m"]),
        efforts: completion_words(script, &["--effort", "--reasoning-effort"]),
    }
}

/// The `compgen -W` word list declared for the first of `flags` that appears as a
/// `case` **pattern** in `script`. A flag mentioned anywhere else (the flat list of
/// every flag name, a comment) is skipped: only an occurrence immediately followed by
/// `)` or `|` is a pattern.
fn completion_words(script: &str, flags: &[&str]) -> Vec<String> {
    for flag in flags {
        for end in flag_token_positions(script, flag) {
            let rest = &script[end..];
            if !rest.starts_with(')') && !rest.starts_with('|') {
                continue;
            }
            let words = compgen_word_list(rest);
            if !words.is_empty() {
                return words;
            }
        }
    }
    Vec::new()
}

/// Read the first `-W '<words>'` (or `-W "<words>"`) list in `rest`, whitespace
/// separated. The `-W` must sit inside **this case arm** — the search stops at the
/// arm's `;;` (or at `esac`, or at [`COMPGEN_WINDOW`] for a generator that uses
/// neither), so an arm that completes filenames yields nothing instead of borrowing
/// the next arm's list. The quoted list itself is read to its closing quote however
/// long it runs.
fn compgen_word_list(rest: &str) -> Vec<String> {
    let arm_end = [rest.find(";;"), rest.find("esac"), Some(COMPGEN_WINDOW)]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(COMPGEN_WINDOW);
    let Some(rel) = window_from(rest, 0, arm_end).find("-W") else {
        return Vec::new();
    };
    let after = rest[rel + "-W".len()..].trim_start();
    let Some(quote) = after.chars().next().filter(|c| *c == '\'' || *c == '"') else {
        return Vec::new();
    };
    let inner = &after[quote.len_utf8()..];
    let Some(close) = inner.find(quote) else {
        return Vec::new();
    };
    split_tokens(&inner[..close], &[' ', '\t', '\n'])
}

// ---------------------------------------------------------------------------
// Source 2 — the settings help topic (#629, ADR-0056)
// ---------------------------------------------------------------------------

/// How many lines past a settings key we read its bullet list before giving up — a
/// generous ceiling on one setting's block (copilot's `model` bullets 27 ids).
const SETTINGS_BLOCK_LINES: usize = 120;

/// Parse a CLI's **settings help topic** (`<bin> help config`) into its offered
/// catalogue (#629, ADR-0056). This is where copilot 1.0.80 actually enumerates its
/// models — its `--help` describes `--model` in prose only, so the #616 reader saw
/// nothing and copilot was served the "no catalogue" fallback while a catalogue
/// existed (#629).
///
/// The shape read is a settings key followed by a bullet list of its allowed values:
///
/// ```text
///   `model`: AI model to use; can be changed with /model or --model.
///     - "claude-opus-5"
///     - "gpt-5.5"
/// ```
///
/// Prose, not a contract: an unexpected layout yields [`Catalogue::default`] and the
/// runner falls through.
pub(crate) fn parse_settings_prose(text: &str) -> Catalogue {
    Catalogue {
        models: settings_values(text, &["model"]),
        efforts: settings_values(text, &["effort", "effortLevel", "reasoningEffort"]),
    }
}

/// The bullet-listed values of the first of `keys` that appears as a settings key
/// line in `text`. Key matching is exact on the un-quoted name, so `model` does not
/// match `subagents.agents.<name>.model` or `contextTier`.
fn settings_values(text: &str, keys: &[&str]) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    for key in keys {
        for (i, line) in lines.iter().enumerate() {
            if !is_settings_key_line(line, key) {
                continue;
            }
            let values = bullet_values(&lines[i + 1..]);
            if !values.is_empty() {
                return values;
            }
        }
    }
    Vec::new()
}

/// Whether `line` declares the setting `key` — `` `key`: … ``, `"key": …`, or a bare
/// `key: …`, at any indent. The decorating backticks/quotes are stripped before the
/// comparison, so the same reader serves a CLI that quotes its keys and one that
/// does not.
fn is_settings_key_line(line: &str, key: &str) -> bool {
    let stripped: String = line.chars().filter(|c| *c != '`' && *c != '"').collect();
    let trimmed = stripped.trim_start();
    trimmed
        .strip_prefix(key)
        .is_some_and(|rest| rest.starts_with(':'))
}

/// Collect the values bulleted under a settings key. Lines before the first bullet
/// are description continuation and are skipped; once bullets start, the first blank
/// or non-bullet line ends the block, so a following key's list is never absorbed.
fn bullet_values(lines: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in lines.iter().take(SETTINGS_BLOCK_LINES) {
        let trimmed = line.trim();
        let bullet = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "));
        match bullet {
            Some(rest) => {
                if let Some(tok) = bullet_value(rest) {
                    if !out.contains(&tok) {
                        out.push(tok);
                    }
                }
            }
            None if out.is_empty() => continue,
            None => break,
        }
    }
    out
}

/// The value a bullet declares: the quoted token when it opens with a quote or a
/// backtick (`- "gpt-5.5": the fast one` ⇒ `gpt-5.5`), else its first whitespace-
/// delimited word. `None` when that is prose rather than an id — which is how a
/// descriptive bullet under some *other* key contributes nothing.
fn bullet_value(rest: &str) -> Option<String> {
    let rest = rest.trim_start();
    let quote = rest.chars().next()?;
    let tok = if matches!(quote, '"' | '\'' | '`') {
        let inner = &rest[quote.len_utf8()..];
        &inner[..inner.find(quote)?]
    } else {
        rest.split_whitespace().next()?
    };
    is_plausible_value(tok).then(|| tok.to_string())
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
        assert!(cat.models.is_empty(), "no model enumeration ⇒ declared absence");
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

    #[test]
    fn a_multibyte_help_does_not_slice_mid_codepoint() {
        // copilot's help prints an em dash. A fixed-byte window that lands inside one
        // used to panic — inside the `/settings` handler, on any machine with that
        // binary installed. Pad so the em dash straddles the WINDOW boundary from the
        // flag, then walk every offset around it.
        for pad in 0..8 {
            let help = format!(
                "  --model <m>{}{}\n  Choices: a, b\n",
                " ".repeat(WINDOW - 4 + pad),
                "— an em dash".repeat(4)
            );
            let _ = parse_help(&help); // must not panic
        }
    }

    // -- The subcommand gate (#629, ADR-0056) ------------------------------------

    #[test]
    fn a_command_list_is_read_bare_or_binary_prefixed() {
        // copilot's shape: bare command names under `Commands:`.
        let copilot = "Commands:\n  completion <shell>   Generate a shell completion script\n  help [topic]         Display help information\n";
        assert!(advertises_subcommand(copilot, "completion"));
        assert!(advertises_subcommand(copilot, "help"));
        // opencode's shape: each command line repeats the binary name.
        let opencode = "Commands:\n  opencode completion    generate shell completion script\n  opencode models        list all available models\n";
        assert!(advertises_subcommand(opencode, "completion"));
        assert!(
            !advertises_subcommand(opencode, "help"),
            "opencode declares no `help` subcommand"
        );
    }

    #[test]
    fn a_binary_that_declares_neither_is_never_asked_for_them() {
        // claude's verbatim shape, abridged. It has no `completion` and no `help`
        // subcommand — and reads either as a PROMPT, opening a session that idles to
        // the probe timeout. This assertion is what keeps a claude re-probe at one
        // subprocess instead of one plus two five-second stalls.
        let claude = "\
Usage: claude [options] [command] [prompt]

Options:
  -h, --help                            Display help for command
  --model <model>                       Model for the session

Commands:
  mcp                                   Configure and manage MCP servers
  plugin|plugins                        Manage Claude Code plugins
  update|upgrade                        Check for updates and install if
                                        available
";
        assert!(!advertises_subcommand(claude, "completion"));
        assert!(
            !advertises_subcommand(claude, "help"),
            "`-h, --help` in the options list is not a `help` subcommand"
        );
    }

    // -- Source 1: the generated completion script (#629, ADR-0056) ---------------

    /// The verbatim shape of `copilot completion bash` 1.0.80: a `case` on the
    /// previous word, one arm per value-taking flag, choices in a `compgen -W` list.
    /// Abridged to five model ids; the structure is what is under test.
    const COPILOT_COMPLETION: &str = r#"
    local ___copilot_required='--add-dir --agent --context --effort --model --env'
    case "$prev" in
        --model)
            COMPREPLY=( $(compgen -W 'auto claude-opus-5 claude-sonnet-4.5 gpt-5.5 kimi-k2.7-code' -- "$cur") )
            return 0
            ;;
        --effort|--reasoning-effort)
            COMPREPLY=( $(compgen -W 'none minimal low medium high xhigh max' -- "$cur") )
            return 0
            ;;
        --context)
            COMPREPLY=( $(compgen -W 'default long_context' -- "$cur") )
            return 0
            ;;
    esac
    ___copilot_flags='--acp --add-dir --agent --effort --model --version'
"#;

    #[test]
    fn a_completion_script_yields_both_axes() {
        // AC #1/#2: the machine-generated source carries the model ids `--help` only
        // describes in prose — including `auto`, copilot's automatic selector, which
        // the settings prose does not list.
        let cat = parse_completion_script(COPILOT_COMPLETION);
        assert_eq!(
            cat.models,
            vec![
                "auto",
                "claude-opus-5",
                "claude-sonnet-4.5",
                "gpt-5.5",
                "kimi-k2.7-code"
            ]
        );
        assert_eq!(
            cat.efforts,
            vec!["none", "minimal", "low", "medium", "high", "xhigh", "max"],
            "an aliased case arm (`--effort|--reasoning-effort`) is still a pattern"
        );
    }

    #[test]
    fn a_flag_named_outside_a_case_arm_is_not_a_choice_list() {
        // `--model` is named twice more in the script (the required-args list, the
        // all-flags list). Only the arm — the occurrence followed by `)` or `|` —
        // declares choices; harvesting from a flat list would offer flag names as
        // model ids.
        let cat = parse_completion_script(COPILOT_COMPLETION);
        assert!(
            !cat.models.iter().any(|m| m.starts_with("--")),
            "flag names must never leak into the catalogue: {:?}",
            cat.models
        );
    }

    #[test]
    fn an_arm_without_a_word_list_never_borrows_a_later_ones() {
        // A flag whose arm completes filenames rather than a fixed set has no offer;
        // the reader must not walk on to the next arm's `-W`.
        let script = "\
    case \"$prev\" in
        --model)
            _filedir
            return 0
            ;;
        --context)
            COMPREPLY=( $(compgen -W 'default long_context' -- \"$cur\") )
            ;;
    esac
";
        assert!(
            parse_completion_script(script).models.is_empty(),
            "no word list on the arm ⇒ no offer, not the neighbour's"
        );
    }

    #[test]
    fn a_script_with_no_such_flag_is_an_empty_catalogue() {
        // What a binary with no `completion` subcommand prints (a usage error), and
        // what a completion script that declares no model choices yields: nothing, so
        // the runner falls through to the next source.
        assert_eq!(
            parse_completion_script("error: unknown command 'completion'"),
            Catalogue::default()
        );
        assert_eq!(parse_completion_script(""), Catalogue::default());
    }

    #[test]
    fn a_double_quoted_word_list_reads_the_same() {
        let script =
            "        --model)\n  COMPREPLY=( $(compgen -W \"opus sonnet\" -- \"$cur\") )\n";
        assert_eq!(
            parse_completion_script(script).models,
            vec!["opus", "sonnet"]
        );
    }

    // -- Source 2: the settings help topic (#629, ADR-0056) ----------------------

    /// The verbatim shape of `copilot help config` 1.0.80: settings keys in
    /// backticks, allowed values bulleted and quoted underneath. Abridged.
    const COPILOT_HELP_CONFIG: &str = r#"Configuration Settings:

  `logLevel`: log level for CLI; defaults to "default". Set to "all" for debug logging.

  `model`: AI model to use for Copilot CLI; can be changed with /model command or --model flag option.
    - "claude-sonnet-5"
    - "claude-opus-4.8-fast"
    - "gpt-5.6-sol"
    - "gemini-3.1-pro-preview"
    - "kimi-k2.7-code"

  `contextTier`: context window tier for tiered-pricing models (e.g., "default" or "long_context").
    - Can also be set with --context flag (overrides persisted setting)

  `subagents.agents.<agent-name>`: per-subagent model, effortLevel, and contextTier selection.
    - Each field can be set to "inherit" to use the parent session's effective value
"#;

    #[test]
    fn the_settings_topic_yields_the_model_ids_the_help_only_describes() {
        // The motivating measurement of #629: copilot's `--help` says "use 'auto' to
        // let Copilot pick" and enumerates nothing, while `help config` bullets every
        // valid id. #616 read only `--help` and concluded copilot had no catalogue.
        let cat = parse_settings_prose(COPILOT_HELP_CONFIG);
        assert_eq!(
            cat.models,
            vec![
                "claude-sonnet-5",
                "claude-opus-4.8-fast",
                "gpt-5.6-sol",
                "gemini-3.1-pro-preview",
                "kimi-k2.7-code"
            ]
        );
        // This topic documents no effort setting ⇒ a declared absence on that axis,
        // which the runner then fills from the next source.
        assert!(cat.efforts.is_empty());
    }

    #[test]
    fn a_settings_block_stops_at_its_own_end() {
        // The `contextTier` bullet ("Can also be set with --context flag") sits right
        // after the model list. Absorbing it would offer prose as a model id.
        let cat = parse_settings_prose(COPILOT_HELP_CONFIG);
        assert!(
            !cat.models.iter().any(|m| m.contains("--context")),
            "the next key's bullets are not this key's values: {:?}",
            cat.models
        );
        assert_eq!(cat.models.len(), 5, "exactly the bulleted ids");
    }

    #[test]
    fn a_key_matches_whole_not_as_a_suffix_or_prefix() {
        // `contextTier` must not answer for `model`, and the dotted per-subagent key
        // that merely *mentions* model in its prose must not either.
        let text = "  `modelFamily`: pick a family.\n    - \"anthropic\"\n\n  `model`: the model.\n    - \"opus\"\n";
        assert_eq!(parse_settings_prose(text).models, vec!["opus"]);
    }

    #[test]
    fn an_unquoted_bullet_list_reads_its_first_word() {
        // Not every CLI quotes its values; a bare bullet with a trailing gloss still
        // yields the id, and a prose bullet yields nothing.
        let text = "  effort: reasoning effort.\n    - low — cheapest\n    - high — slowest\n";
        assert_eq!(parse_settings_prose(text).efforts, vec!["low", "high"]);
    }

    #[test]
    fn a_settings_topic_with_no_keys_is_an_empty_catalogue() {
        assert_eq!(
            parse_settings_prose("error: unknown help topic 'config'"),
            Catalogue::default()
        );
    }

    // -- The per-axis fold across sources (#629, ADR-0056) -----------------------

    #[test]
    fn each_axis_is_owned_by_the_first_source_that_offers_it() {
        // The copilot case with an older binary: models come from the settings topic,
        // efforts from `--help`, and neither overwrites an axis already answered.
        let mut cat = parse_settings_prose(COPILOT_HELP_CONFIG);
        assert!(!cat.is_complete(), "models only ⇒ the walk continues");
        cat.fill_missing_from(parse_help(
            "  --model <m> Set the AI model (use 'auto')\n  --effort <e> (choices: \"low\", \"high\")\n",
        ));
        assert_eq!(
            cat.models.first().map(String::as_str),
            Some("claude-sonnet-5")
        );
        assert_eq!(cat.efforts, vec!["low", "high"]);
        assert!(cat.is_complete(), "both axes answered ⇒ the walk stops");
    }

    #[test]
    fn a_filled_axis_is_never_overwritten_by_a_later_source() {
        let mut cat = Catalogue {
            models: vec!["preferred".into()],
            efforts: Vec::new(),
        };
        cat.fill_missing_from(Catalogue {
            models: vec!["fallback".into()],
            efforts: vec!["low".into()],
        });
        assert_eq!(cat.models, vec!["preferred"]);
        assert_eq!(cat.efforts, vec!["low"]);
    }
}
