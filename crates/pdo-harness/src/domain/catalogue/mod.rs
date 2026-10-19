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
//! [`crate::adapters::binary_path::binary_available`], the one place that reads the
//! environment. The two responsibilities meet in `lib`'s settings view and
//! the boot probe.
//!
//! ## Five sources, machine-generated first (#629, #705, #798, ADR-0056)
//!
//! A binary does not always print its enumeration where a `--help` reader looks.
//! Measured on `copilot` 1.0.80: `--help` enumerates the effort stops but describes
//! `--model` in prose, while the ids live in two other places. Measured on `pi`
//! 0.85.1: `--help` describes `--model` as a pattern but a `--list-models` flag prints
//! the whole embedded catalogue as a table. So the module exposes **five readers**,
//! which the runner tries in preference order:
//!
//! 1. [`parse_completion_script`] — the shell-completion script (`<bin> completion
//!    bash`). Machine-**generated** from the CLI's own declared choices, so it is the
//!    most stable of the five and the one ADR-0056 prefers.
//! 2. [`parse_list_models`] — the model table (`<bin> --list-models`), generated from
//!    the binary's embedded catalogue, carrying each model's **context window** (#705,
//!    ADR-0056 §1 as amended by #702).
//! 3. [`parse_settings_prose`] — the settings help topic (`<bin> help config`), where
//!    a CLI documents each setting and bullets its allowed values.
//! 4. [`parse_help`] — the `--help` reader of #616.
//! 5. the **RPC model-effort table** (#798) — a `--mode rpc` session whose models are
//!    listed (`get_available_models`) and then interrogated one by one (`set_model` +
//!    `get_available_thinking_levels`), so the levels come from the binary's own
//!    report *per model*, not from a model-name rule nor a `reasoning` boolean.
//!    Measured on pi 0.85.1: `openrouter/stealth/union-alpha` reports `[off]` where
//!    `openrouter/openai/gpt-5.4` reports `[off, low, medium, high, xhigh]` — no name
//!    rule predicts that. It is the only source that answers the per-model axis
//!    ([`Catalogue::model_efforts`]), and the most expensive by far, so it is gated
//!    on the binary declaring every flag the session passes ([`rpc_probe_allowed`])
//!    and bounded in time and model count.
//!
//! Each axis (models, efforts, per-model efforts) takes the **highest-preference
//! source that offers one** — see [`Catalogue::fill_missing_from`]. The two richer
//! sources are only *run* against a binary whose `--help` declares them
//! ([`advertises_subcommand`]); the runner's doc says why that gate is load-bearing.
//!
//! ## Best-effort, never a contract (ADR-0053 §Limites)
//!
//! None of the five is an API: `--help` and `help config` are prose, a completion
//! script is generated bash, a model table is aligned text. Each reader scans for the enumerations a CLI
//! conventionally prints beside `--model` / `--effort` (`[a|b|c]`, `<a|b|c>`,
//! `Choices: a, b, c`, `One of: …`, a bare parenthesised `(a, b, c)`, a run of quoted
//! ids `'a', 'b', 'c'`). Every reader is best-effort and can go **blind** to a
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
mod help;
mod sources;

pub use help::parse_help;
pub use sources::{
    advertises_flag, advertises_subcommand, parse_completion_script, parse_list_models,
    parse_rpc_models, parse_rpc_thinking_levels, parse_settings_prose, rpc_probe_allowed,
    RPC_HYGIENE_FLAGS,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    /// Offered model ids (`--model` enumeration), verbatim as the binary spells
    /// them. A value the picker does not carry is still accepted (ADR-0001): this
    /// is an offer, never a guard.
    pub models: Vec<String>,
    /// Offered effort levels (`--effort` enumeration). Empty ⇒ this binary exposes
    /// no effort axis; the client greys the picker.
    pub efforts: Vec<String>,
    /// The **context window** a source published beside a model id, keyed by the id
    /// exactly as it appears in [`Self::models`] (#705: `pi --list-models` prints one
    /// per row — `200K`, `1M`). A hint the picker shows next to the id, verbatim as
    /// the binary spells it; never parsed into a number, never a guard. Empty for a
    /// source that names no window (every source but the model table). Rides with
    /// `models`: [`Self::fill_missing_from`] moves the two together, so a window can
    /// never describe a model from another source.
    pub model_contexts: std::collections::BTreeMap<String, String>,
    /// The **thinking levels offered per model** (#798), keyed by the model id
    /// exactly as [`Self::models`] spells it (`provider/id` — the rendering
    /// `--list-models` offers), to the levels the binary itself reports for that
    /// model (pi's RPC `get_available_thinking_levels`), verbatim and in the
    /// binary's order. A **missing key** is *unknown*: the client falls back to the
    /// global [`Self::efforts`] axis. A **present** key — an empty vec included — is
    /// *known*: the model offers exactly those levels (empty: none). Never inferred
    /// from the model's name or a `reasoning` boolean: measured on pi 0.85.1,
    /// `openrouter/stealth/union-alpha` reports `[off]` where
    /// `openrouter/openai/gpt-5.4` reports `[off, low, medium, high, xhigh]`, and no
    /// name rule predicts that. Filled only by the RPC source; every other source
    /// leaves it empty (⇒ every model unknown, the pre-#798 behaviour).
    pub model_efforts: std::collections::BTreeMap<String, Vec<String>>,
}

impl Catalogue {
    /// Whether the binary enumerates an effort axis — the **served** fact the
    /// client's effort-picker greying now reads instead of a hard-coded map
    /// (ADR-0053, closes the client-side `HARNESS_HAS_EFFORT`). A harness whose
    /// launch template carries an `{effort}` hole but whose help enumerates none
    /// (or vice-versa) is folded in by the caller — see the settings view.
    pub fn has_effort_axis(&self) -> bool {
        !self.efforts.is_empty() || !self.model_efforts.is_empty()
    }

    /// Fold a lower-preference source into this one, **per axis** (#629, ADR-0056).
    /// An axis this catalogue already offers is kept; an axis it lacks is taken from
    /// `other`. Axis-wise and not whole-catalogue, because the sources disagree about
    /// what they cover: copilot's completion script carries both axes, its
    /// `help config` only models, its `--help` only efforts.
    ///
    /// The caller folds in **preference order**, so "already offered" means "answered
    /// by a source that outranks this one" — not "answered by whichever ran first".
    /// The per-model effort table folds like a third axis: only the RPC source
    /// answers it, so in practice the fold moves it or nothing.
    pub fn fill_missing_from(&mut self, other: Catalogue) {
        if self.models.is_empty() {
            self.models = other.models;
            self.model_contexts = other.model_contexts;
        }
        if self.efforts.is_empty() {
            self.efforts = other.efforts;
        }
        if self.model_efforts.is_empty() {
            self.model_efforts = other.model_efforts;
        }
    }

    /// Whether both axes are filled — the runner's short-circuit: with models *and*
    /// efforts already answered, a lower-preference source has nothing left to
    /// contribute and its subprocess is not spent.
    pub fn is_complete(&self) -> bool {
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
pub struct CachedCatalogue {
    pub version: Option<String>,
    pub catalogue: Catalogue,
    /// When the catalogue ladder last ran for it (RFC 3339 UTC): a version
    /// re-check that finds the same version leaves it alone. `None` for a
    /// catalogue never probed (an uninstalled binary).
    pub probed_at: Option<String>,
}
#[cfg(test)]
mod tests {
    use super::help::*;
    use super::*;

    /// The real `pi --help` of 0.85.1, captured verbatim (#705). Beside copilot's
    /// fixtures in spirit: the shapes measured on the installed binary, not invented.
    const PI_HELP: &str = include_str!("../../../tests/fixtures/catalogue/pi-0.85.1-help.txt");
    /// The real `pi --list-models` of 0.85.1, abridged to its alias rows plus a few
    /// concrete ones (the full table is 366 rows on this host's providers).
    const PI_LIST_MODELS: &str =
        include_str!("../../../tests/fixtures/catalogue/pi-0.85.1-list-models.txt");

    #[test]
    fn pi_help_declares_list_models_but_neither_subcommand_source() {
        // ADR-0056 §1 bis: only what the binary announces is run. pi's `Commands:`
        // block names install/remove/update/list/config/auth — never `completion` nor
        // `help` — and its options name `--list-models`.
        assert!(advertises_flag(PI_HELP, "--list-models"));
        assert!(!advertises_subcommand(PI_HELP, "completion"));
        assert!(!advertises_subcommand(PI_HELP, "help"));
        // A flag it does not have is not declared — the claude hazard, mirrored.
        assert!(!advertises_flag(PI_HELP, "--list-efforts"));
        assert!(!advertises_flag(
            "  --model <m>  the model\n",
            "--list-models"
        ));
        // An alias-joined option line still declares its long flag.
        assert!(advertises_flag(
            "  --print, -p   Non-interactive\n",
            "--print"
        ));
    }

    #[test]
    fn pi_help_enumerates_its_thinking_levels_as_the_effort_axis() {
        // `--thinking <level>  Set thinking level: off, minimal, low, medium, high,
        // xhigh, max` — pi's effort axis, read off `--help` (AC: efforts = the
        // `--thinking` line). `--model <pattern>` is prose, so no model list here.
        let cat = parse_help(PI_HELP);
        assert_eq!(
            cat.efforts,
            vec!["off", "minimal", "low", "medium", "high", "xhigh", "max"]
        );
        assert!(
            cat.models.is_empty(),
            "pi's --help describes --model as a pattern, not a list: {:?}",
            cat.models
        );
    }

    #[test]
    fn pi_list_models_offers_provider_slash_model_with_the_context_window() {
        let cat = parse_list_models(PI_LIST_MODELS);
        // Aliases (the `~…-latest` rows) are offered like any model, verbatim.
        assert_eq!(cat.models[0], "openrouter/~anthropic/claude-fable-latest");
        assert!(cat
            .models
            .iter()
            .any(|m| m == "openrouter/anthropic/claude-sonnet-4.5"));
        assert_eq!(cat.models.len(), 17, "one offer per row, header excluded");
        // The context window rides beside each id, as the binary spells it.
        assert_eq!(
            cat.model_contexts
                .get("openrouter/anthropic/claude-opus-4")
                .map(String::as_str),
            Some("200K")
        );
        assert_eq!(
            cat.model_contexts
                .get("openrouter/~anthropic/claude-sonnet-latest")
                .map(String::as_str),
            Some("1M")
        );
        assert_eq!(cat.model_contexts.len(), cat.models.len());
        // The table says nothing about effort.
        assert!(cat.efforts.is_empty());
    }

    #[test]
    fn a_model_table_without_the_expected_columns_is_no_offer() {
        assert_eq!(parse_list_models(""), Catalogue::default());
        assert_eq!(
            parse_list_models("Usage: thing --list-models\n  nothing here\n"),
            Catalogue::default()
        );
        // Header only: no rows, no offer.
        assert_eq!(
            parse_list_models("provider  model  context\n"),
            Catalogue::default()
        );
    }

    #[test]
    fn model_contexts_travel_with_the_models_they_describe() {
        // ADR-0056 §3, per axis: when a higher-preference source already answered
        // the model axis, a lower one's windows must not be grafted onto its ids.
        let mut top = Catalogue {
            models: vec!["from-completion".into()],
            ..Default::default()
        };
        let table = parse_list_models(PI_LIST_MODELS);
        top.fill_missing_from(table.clone());
        assert_eq!(top.models, vec!["from-completion"]);
        assert!(top.model_contexts.is_empty());
        // And when the model axis is empty, the windows arrive with the ids.
        let mut empty = Catalogue::default();
        empty.fill_missing_from(table);
        assert_eq!(empty.models.len(), 17);
        assert_eq!(empty.model_contexts.len(), 17);
    }

    #[test]
    fn the_pi_help_declares_everything_the_rpc_probe_passes() {
        // #798: the real pi 0.85.1 `--help` passes the gate — its `--mode` line
        // offers `rpc`, and every hygiene flag is declared. This is what lets the
        // runner open a bounded RPC session on the installed binary.
        assert!(rpc_probe_allowed(PI_HELP));
    }

    #[test]
    fn a_mode_line_without_rpc_or_a_missing_hygiene_flag_refuses_the_rpc_probe() {
        // The gate is per flag, not vibes: `--mode` that enumerates no `rpc` is a
        // different binary shape; so is one missing hygiene flag (we never pass an
        // argv token the binary didn't announce — the claude hazard).
        let no_rpc = "  --mode <mode>                  Output mode: text (default), json\n";
        let mut flags = String::from("Options:\n");
        for flag in RPC_HYGIENE_FLAGS {
            flags.push_str(&format!("  {flag}   off\n"));
        }
        let hygiene_only = format!("{flags}  --mode <mode>  Output mode: text, json, or rpc\n");
        assert!(!rpc_probe_allowed(no_rpc));
        assert!(rpc_probe_allowed(&hygiene_only));
        // One flag short and the whole session is refused.
        let mut short = flags.clone();
        short.push_str("  --mode <mode>  Output mode: text, json, or rpc\n");
        let dropped = short.replace("--no-approve", "--no-other-flag");
        assert!(!rpc_probe_allowed(&dropped));
    }

    #[test]
    fn rpc_model_responses_parse_to_verbatim_provider_id_pairs() {
        // The measured shape of pi 0.85.1's `get_available_models` response
        // (abridged to the issue's two models): `data.models`, each a Model object
        // with `provider` and `id`.
        let line = r#"{"id":"1","type":"response","command":"get_available_models","success":true,"data":{"models":[{"id":"openai/gpt-5.4","name":"GPT 5.4","provider":"openrouter","reasoning":true},{"id":"stealth/union-alpha","name":"Union","provider":"openrouter","reasoning":false}]}}"#;
        assert_eq!(
            parse_rpc_models(line),
            vec![
                ("openrouter".to_string(), "openai/gpt-5.4".to_string()),
                ("openrouter".to_string(), "stealth/union-alpha".to_string()),
            ]
        );
    }

    #[test]
    fn anything_but_a_successful_models_response_is_no_models() {
        // Events, failures, other commands, garbage: all an empty vec, so the
        // runner's skip-and-wait loop never scavenges the wrong line.
        assert!(parse_rpc_models("not json at all").is_empty());
        assert!(parse_rpc_models(r#"{"type":"agent_start"}"#).is_empty());
        assert!(parse_rpc_models(
            r#"{"id":"1","type":"response","command":"get_available_models","success":false,"error":"nope"}"#
        )
        .is_empty());
        assert!(parse_rpc_models(
            r#"{"id":"1","type":"response","command":"set_model","success":true,"data":{}}"#
        )
        .is_empty());
        // A model row missing a field is dropped, not half-kept.
        assert!(parse_rpc_models(
            r#"{"id":"1","type":"response","command":"get_available_models","success":true,"data":{"models":[{"id":"no-provider"}]}}"#
        )
        .is_empty());
    }

    #[test]
    fn rpc_thinking_levels_are_verbatim_in_the_binary_s_order() {
        // The issue's exact repro, byte for byte from the binary's report.
        let gpt54 = r#"{"id":"t","type":"response","command":"get_available_thinking_levels","success":true,"data":{"levels":["off","low","medium","high","xhigh"]}}"#;
        assert_eq!(
            parse_rpc_thinking_levels(gpt54),
            Some(vec![
                "off".to_string(),
                "low".to_string(),
                "medium".to_string(),
                "high".to_string(),
                "xhigh".to_string(),
            ])
        );
        // A non-reasoning model still answers — `[off]` is a KNOWN offer of one
        // level, not an unknown: `parse_rpc_thinking_levels` must not collapse it
        // to None (that would silently fall back to the global axis).
        let union_alpha = r#"{"id":"t","type":"response","command":"get_available_thinking_levels","success":true,"data":{"levels":["off"]}}"#;
        assert_eq!(
            parse_rpc_thinking_levels(union_alpha),
            Some(vec!["off".to_string()])
        );
        // An empty list is present-and-empty: known to offer nothing.
        assert_eq!(
            parse_rpc_thinking_levels(
                r#"{"id":"t","type":"response","command":"get_available_thinking_levels","success":true,"data":{"levels":[]}}"#
            ),
            Some(Vec::new())
        );
    }

    #[test]
    fn anything_but_a_successful_levels_response_is_none() {
        assert_eq!(parse_rpc_thinking_levels("garbage"), None);
        assert_eq!(
            parse_rpc_thinking_levels(r#"{"type":"message_start"}"#),
            None
        );
        assert_eq!(
            parse_rpc_thinking_levels(
                r#"{"id":"t","type":"response","command":"get_available_thinking_levels","success":false,"error":"no model"}"#
            ),
            None,
            "a failure leaves the model UNKNOWN, not known-empty"
        );
    }

    #[test]
    fn malformed_levels_are_unknown_not_an_authoritative_subset() {
        for levels in [serde_json::json!([null]), serde_json::json!(["off", 42])] {
            let response = serde_json::json!({
                "type": "response", "command": "get_available_thinking_levels",
                "success": true, "data": {"levels": levels},
            });
            assert_eq!(parse_rpc_thinking_levels(&response.to_string()), None);
        }
    }

    #[test]
    fn model_efforts_alone_declare_an_effort_axis() {
        let catalogue = Catalogue {
            model_efforts: [("provider/model".into(), vec!["off".into()])].into(),
            ..Default::default()
        };
        assert!(catalogue.has_effort_axis());
    }

    #[test]
    fn model_efforts_fold_like_a_third_axis() {
        // `fill_missing_from` moves the per-model table only when this catalogue
        // has none — an already-answered axis is never overwritten, and an empty
        // per-model vec is an ANSWER (it must survive the fold, not read as
        // "unanswered").
        let mut with_answer = Catalogue {
            model_efforts: [("p/m".to_string(), Vec::new())].into_iter().collect(),
            ..Default::default()
        };
        with_answer.fill_missing_from(Catalogue {
            model_efforts: [("other/m".to_string(), vec!["off".to_string()])]
                .into_iter()
                .collect(),
            ..Default::default()
        });
        assert_eq!(with_answer.model_efforts.len(), 1);
        assert!(with_answer.model_efforts.contains_key("p/m"));

        let mut empty = Catalogue::default();
        empty.fill_missing_from(Catalogue {
            model_efforts: [("p/m".to_string(), vec!["off".to_string()])]
                .into_iter()
                .collect(),
            ..Default::default()
        });
        assert_eq!(
            empty.model_efforts.get("p/m").map(Vec::as_slice),
            Some(["off".to_string()].as_slice())
        );
    }

    #[test]
    fn parses_a_bracketed_pipe_model_list() {
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
        assert!(cat.efforts.is_empty());
        assert!(!cat.has_effort_axis());
    }

    #[test]
    fn parses_a_keyword_effort_list_and_a_keyword_model_list() {
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

    #[test]
    fn a_command_list_is_read_bare_or_binary_prefixed() {
        let copilot = "Commands:\n  completion <shell>   Generate a shell completion script\n  help [topic]         Display help information\n";
        assert!(advertises_subcommand(copilot, "completion"));
        assert!(advertises_subcommand(copilot, "help"));
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

    #[test]
    fn each_axis_is_owned_by_the_first_source_that_offers_it() {
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
            ..Default::default()
        };
        cat.fill_missing_from(Catalogue {
            models: vec!["fallback".into()],
            efforts: vec!["low".into()],
            ..Default::default()
        });
        assert_eq!(cat.models, vec!["preferred"]);
        assert_eq!(cat.efforts, vec!["low"]);
    }
}
