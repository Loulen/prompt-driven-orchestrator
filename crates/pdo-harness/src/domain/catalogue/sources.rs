//! The richer sources a binary may declare (#629, #705, #798, ADR-0056):
//! completion script, model table, settings prose, RPC model-effort report.

use super::help::*;
use super::Catalogue;

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
pub fn advertises_subcommand(help: &str, name: &str) -> bool {
    help.lines().any(|line| {
        let mut words = line.split_whitespace();
        let (first, second) = (words.next(), words.next());
        first == Some(name) || second == Some(name)
    })
}

/// Whether `help` (a binary's `--help`) declares the flag `flag` (#705, ADR-0056
/// §1 bis). PURE; the runner uses it to decide whether `--list-models` may be run at
/// all. The same load-bearing gate as [`advertises_subcommand`]: `claude` has no
/// `--list-models` and would read an unknown flag as an error at best, a prompt at
/// worst — a flag is only run when the binary itself announces it.
///
/// A flag is declared when it opens an option line — `  --list-models [search]  List
/// available models` — i.e. it is the first word of a line, possibly followed by a
/// `,`-joined alias (`--print, -p`). Loose on purpose, like its sibling: a false
/// positive costs one bounded probe, a false negative costs the catalogue.
pub fn advertises_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|w| w.trim_end_matches(',') == flag)
    })
}

/// Parse a binary's **model table** (`<bin> --list-models`) into its offered
/// catalogue (#705, ADR-0056 §1 as amended by #702). Generated from the embedded
/// catalogue, so preferred over prose; carries the **context window** per model,
/// which no other source does.
///
/// The shape read is `pi` 0.85.1's: a header line naming its columns, then one row
/// per model, whitespace-aligned:
///
/// ```text
/// provider    model                             context  max-out  thinking  images
/// openrouter  ~anthropic/claude-sonnet-latest   1M       128K     yes       yes
/// openrouter  anthropic/claude-sonnet-4.5       1M       64K      yes       yes
/// ```
///
/// Each model is offered as **`provider/model`** — the form `--model` accepts with no
/// `--provider` (`pi --model openai/gpt-4o`) — an alias row (`~…-latest`) included,
/// verbatim. The `context` column, when the header names one, is kept beside the id
/// in [`Catalogue::model_contexts`]. A table with no `provider`/`model` columns, or
/// no rows, yields [`Catalogue::default`] — the runner falls through. This reader
/// never answers the effort axis: `--list-models` says nothing about it.
pub fn parse_list_models(table: &str) -> Catalogue {
    let mut lines = table.lines().filter(|l| !l.trim().is_empty());
    let Some(header) = lines.next() else {
        return Catalogue::default();
    };
    let columns: Vec<&str> = header.split_whitespace().collect();
    let col = |name: &str| columns.iter().position(|c| c.eq_ignore_ascii_case(name));
    let (Some(provider_col), Some(model_col)) = (col("provider"), col("model")) else {
        return Catalogue::default();
    };
    let context_col = col("context");

    let mut out = Catalogue::default();
    for line in lines {
        let cells: Vec<&str> = line.split_whitespace().collect();
        let (Some(provider), Some(model)) = (cells.get(provider_col), cells.get(model_col)) else {
            continue;
        };
        if !is_plausible_model_row_cell(provider) || !is_plausible_model_row_cell(model) {
            continue;
        }
        let id = format!("{provider}/{model}");
        if out.models.contains(&id) {
            continue;
        }
        if let Some(ctx) = context_col.and_then(|c| cells.get(c)) {
            if !ctx.is_empty() && *ctx != "-" {
                out.model_contexts.insert(id.clone(), ctx.to_string());
            }
        }
        out.models.push(id);
    }
    out
}

/// A provider or model cell of the model table: [`is_plausible_value`] plus the `~`
/// an alias row opens with (`~anthropic/claude-sonnet-latest`) and the `@`/`+` some
/// provider ids carry. Kept apart from the prose readers' filter so a `~` never
/// starts passing as a word harvested from help text.
pub(super) fn is_plausible_model_row_cell(cell: &str) -> bool {
    !cell.is_empty()
        && cell.len() <= 80
        && cell.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/' | ':' | '~' | '@' | '+')
        })
}

/// The flags the RPC model-effort probe passes besides `--mode rpc` (#798). Every
/// one must be declared by the binary's `--help` before the probe may pass it: the
/// same load-bearing gate as [`advertises_subcommand`] — an unknown flag is at best
/// an error, at worst a prompt, and the session runs inside a `/settings` response.
/// Together they make the session **inert**: no persistence, no extensions, no
/// skills, no prompt templates, no themes, no context files, no network, no trust
/// of project-local files — a discovery probe, not a working session.
pub const RPC_HYGIENE_FLAGS: &[&str] = &[
    "--no-session",
    "--no-extensions",
    "--no-skills",
    "--no-prompt-templates",
    "--no-themes",
    "--no-context-files",
    "--offline",
    "--no-approve",
];

/// Whether `help` (a binary's `--help`) declares everything the RPC model-effort
/// probe passes (#798): a `--mode` option whose own line offers `rpc`, and every
/// [`RPC_HYGIENE_FLAGS`]. PURE; the runner uses it as its gate. Loose on purpose,
/// like its siblings: a false positive costs one bounded probe that finds nothing,
/// a false negative costs the per-model axis (the global efforts still stand).
pub fn rpc_probe_allowed(help: &str) -> bool {
    let mode_offers_rpc = help.lines().any(|line| {
        line.split_whitespace()
            .next()
            .is_some_and(|w| w.trim_end_matches(',') == "--mode")
            && line.split_whitespace().any(|w| w == "rpc")
    });
    mode_offers_rpc && RPC_HYGIENE_FLAGS.iter().all(|f| advertises_flag(help, f))
}

/// Whether `v` (a parsed RPC line) is a **successful response** of `command` — the
/// envelope every RPC reader requires before it looks at `data`. An event, a
/// failure, or another command's response is refused whole: a parser never
/// scavenges a payload out of the wrong message.
pub(super) fn rpc_response_is(v: &serde_json::Value, command: &str) -> bool {
    v["type"].as_str() == Some("response")
        && v["command"].as_str() == Some(command)
        && v["success"].as_bool() == Some(true)
}

/// The (provider, model id) pairs one `get_available_models` RPC response line
/// offers (#798). PURE: a JSON line in, pairs out. Anything else — an event, a
/// failure response, another command, malformed JSON — is an empty vec: the runner
/// skips and waits, bounded, for the line it asked for. Each pair keeps its two
/// fields verbatim; the runner composes the catalogue key as `provider/id`, the
/// same rendering `--list-models` uses, so the keys meet [`Catalogue::models`]
/// entries without translation.
pub fn parse_rpc_models(line: &str) -> Vec<(String, String)> {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return Vec::new();
    };
    if !rpc_response_is(&v, "get_available_models") {
        return Vec::new();
    }
    let Some(models) = v["data"]["models"].as_array() else {
        return Vec::new();
    };
    models
        .iter()
        .filter_map(|m| {
            let provider = m["provider"].as_str()?;
            let id = m["id"].as_str()?;
            (is_plausible_model_row_cell(provider) && is_plausible_model_row_cell(id))
                .then(|| (provider.to_string(), id.to_string()))
        })
        .collect()
}

/// The thinking levels one `get_available_thinking_levels` RPC response reports
/// (#798), verbatim and in the binary's order — `Some` also for an **empty** list
/// (a present key that says the model offers none). `None` for anything that is
/// not a successful response of that command, so a failure response leaves the
/// model *unknown* (the global fallback) instead of *known-empty*.
pub fn parse_rpc_thinking_levels(line: &str) -> Option<Vec<String>> {
    let v = serde_json::from_str::<serde_json::Value>(line).ok()?;
    if !rpc_response_is(&v, "get_available_thinking_levels") {
        return None;
    }
    let levels = v["data"]["levels"].as_array()?;
    levels
        .iter()
        .map(|level| {
            let level = level.as_str()?;
            is_plausible_level(level).then(|| level.to_string())
        })
        .collect()
}

/// A level token of the RPC table: short, whitespace-free, non-empty. Looser than
/// [`is_plausible_value`] (a level id is a single word — `off`, `xhigh`) but strict
/// enough that a mis-parsed sentence can never become a level.
pub(super) fn is_plausible_level(tok: &str) -> bool {
    !tok.is_empty() && tok.len() <= 32 && tok.chars().all(|c| !c.is_whitespace() && !c.is_control())
}

/// Backstop bound on one case arm's body, for a generator that ends its arms some
/// way other than `;;` / `esac`. The `compgen -W` list *itself* may be far longer
/// (copilot's is ~700 chars); this bounds only where we look for the `-W`.
pub(super) const COMPGEN_WINDOW: usize = 200;

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
pub fn parse_completion_script(script: &str) -> Catalogue {
    Catalogue {
        models: completion_words(script, &["--model", "-m"]),
        efforts: completion_words(script, EFFORT_FLAGS),
        ..Default::default()
    }
}

/// The `compgen -W` word list declared for the first of `flags` that appears as a
/// `case` **pattern** in `script`. A flag mentioned anywhere else (the flat list of
/// every flag name, a comment) is skipped: only an occurrence immediately followed by
/// `)` or `|` is a pattern.
pub(super) fn completion_words(script: &str, flags: &[&str]) -> Vec<String> {
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
pub(super) fn compgen_word_list(rest: &str) -> Vec<String> {
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

/// How many lines past a settings key we read its bullet list before giving up — a
/// generous ceiling on one setting's block (copilot's `model` bullets 27 ids).
pub(super) const SETTINGS_BLOCK_LINES: usize = 120;

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
pub fn parse_settings_prose(text: &str) -> Catalogue {
    Catalogue {
        models: settings_values(text, &["model"]),
        efforts: settings_values(text, &["effort", "effortLevel", "reasoningEffort"]),
        ..Default::default()
    }
}

/// The bullet-listed values of the first of `keys` that appears as a settings key
/// line in `text`. Key matching is exact on the un-quoted name, so `model` does not
/// match `subagents.agents.<name>.model` or `contextTier`.
pub(super) fn settings_values(text: &str, keys: &[&str]) -> Vec<String> {
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
pub(super) fn is_settings_key_line(line: &str, key: &str) -> bool {
    let stripped: String = line.chars().filter(|c| *c != '`' && *c != '"').collect();
    let trimmed = stripped.trim_start();
    trimmed
        .strip_prefix(key)
        .is_some_and(|rest| rest.starts_with(':'))
}

/// Collect the values bulleted under a settings key. Lines before the first bullet
/// are description continuation and are skipped; once bullets start, the first blank
/// or non-bullet line ends the block, so a following key's list is never absorbed.
pub(super) fn bullet_values(lines: &[&str]) -> Vec<String> {
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
pub(super) fn bullet_value(rest: &str) -> Option<String> {
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
pub(super) fn is_plausible_value(tok: &str) -> bool {
    !tok.is_empty()
        && tok.len() <= 60
        && tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '/' | ':'))
}
