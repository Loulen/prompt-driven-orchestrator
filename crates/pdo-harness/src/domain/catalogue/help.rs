//! The `--help` reader (#616): the enumeration a CLI prints beside `--model`
//! and `--effort`.

use super::sources::is_plausible_value;
use super::Catalogue;

/// Hard cap on how far past a `--model` / `--effort` flag token we scan. The real
/// bound is [`option_block`] — the next option line — so this only keeps a help text
/// with no option structure at all (one long paragraph) from being scanned whole.
/// Generous on purpose: `claude` spells its `--model` offer over five wrapped lines.
pub(super) const WINDOW: usize = 800;

/// Left margin, in columns, at or below which a line starting with `-` opens a **new
/// option** rather than continuing the current one. Every CLI in play prints its
/// options at column 2..=6 and wraps their descriptions far to the right (column 40),
/// so this separates the two without parsing the layout.
pub(super) const OPTION_INDENT: usize = 8;

/// Parse a binary's `--help` text into its offered catalogue. PURE and
/// harness-agnostic: every CLI in play spells the flags `--model` / `--effort`, and
/// the enumeration conventions are shared, so the same reader serves `claude`,
/// `copilot`, and any disk-declared harness. A harness that prints no enumeration
/// (opencode) yields [`Catalogue::default`] — the free-text fallback.
pub fn parse_help(help: &str) -> Catalogue {
    Catalogue {
        models: extract_choices(help, &["--model", "-m"]),
        efforts: extract_choices(help, EFFORT_FLAGS),
        ..Default::default()
    }
}

/// The flags a CLI spells its effort axis under. `--thinking` is `pi`'s (#705,
/// measured on 0.85.1: `--thinking <level>  Set thinking level: off, minimal, low,
/// medium, high, xhigh, max`); it fills the descriptor's `{effort}` hole exactly as
/// `--effort` does for `claude`, so it is the same axis under another name.
pub(super) const EFFORT_FLAGS: &[&str] = &["--effort", "--reasoning-effort", "--thinking"];

/// Extract the enumerated choices printed beside the first of `flags` that appears
/// in `help`. Returns an empty vec when no flag matches or the matched flag carries
/// no enumeration (a bare `<model>` placeholder, e.g. opencode's free-text model).
pub(super) fn extract_choices(help: &str, flags: &[&str]) -> Vec<String> {
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
pub(super) fn option_block(help: &str, from: usize) -> &str {
    let end = floor_boundary(help, from.saturating_add(WINDOW));
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
pub(super) fn opens_a_new_option(line: &str) -> bool {
    let trimmed = line.trim_start();
    trimmed.starts_with('-') && line.len() - trimmed.len() <= OPTION_INDENT
}

/// The `len`-byte window of `text` starting at `from`, **snapped down to a char
/// boundary** at both ends. Help text is not ASCII (copilot's prints an em dash), so
/// a naive `&text[a..a + len]` can slice mid-codepoint and panic — inside the
/// `/settings` handler, on a machine whose only sin is having that binary installed.
/// Truncating the window one character early is the harmless failure here.
pub(super) fn window_from(text: &str, from: usize, len: usize) -> &str {
    let start = floor_boundary(text, from);
    let end = floor_boundary(text, start.saturating_add(len).min(text.len()));
    &text[start..end]
}

/// The largest byte index `<= i` that is a char boundary of `text` (`i` clamped into
/// range first).
pub(super) fn floor_boundary(text: &str, i: usize) -> usize {
    let mut i = i.min(text.len());
    while i > 0 && !text.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Find `flag` as a **whole token** in `help` and return the byte index just past
/// its first occurrence. See [`flag_token_positions`].
pub(super) fn find_flag_token(help: &str, flag: &str) -> Option<usize> {
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
pub(super) fn flag_token_positions(text: &str, flag: &str) -> Vec<usize> {
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

/// Read the first enumeration in `window`, whichever convention spells it: a
/// bracketed pipe list (`[a|b|c]` / `<a|b|c>`), a keyword list (`Choices: a, b, c`),
/// a bare parenthesised list (`(low, medium, high)`), or a run of quoted ids
/// (`'fable', 'opus', 'sonnet'`). The **earliest** match wins, so a `<model>`
/// placeholder immediately followed by `Choices: …` still resolves, and a prose
/// parenthetical that precedes the real list never displaces it.
pub(super) fn extract_enum(window: &str) -> Vec<String> {
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
pub(super) fn bracketed_pipe_list(window: &str) -> Option<(usize, Vec<String>)> {
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
pub(super) const KEYS: &[&str] = &[
    "choices:",
    "one of:",
    "values:",
    "allowed:",
    "supported:",
    // `pi` 0.85.1: `Set thinking level: off, minimal, low, medium, high, xhigh, max`.
    "level:",
];

/// A flat list ends at its line (`Choices: a, b, c`). But a CLI may **wrap** a long
/// enumeration across continuation lines *inside a parenthesis* — copilot 1.0.80
/// prints `… (choices:\n "none", "minimal",\n … "max")`. When the keyword sits inside
/// an unclosed `(`, the list is read to the matching `)` instead, so the wrapped
/// values are not truncated at the first newline. Stopping at the line there would
/// yield an empty axis and silently drop copilot's seven effort stops (#616 FP).
pub(super) fn keyword_list(window: &str) -> Option<(usize, Vec<String>)> {
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
pub(super) fn parenthesised_list(window: &str) -> Option<(usize, Vec<String>)> {
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
pub(super) fn quoted_id_run(window: &str) -> Option<(usize, Vec<String>)> {
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
pub(super) fn split_tokens(s: &str, seps: &[char]) -> Vec<String> {
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
