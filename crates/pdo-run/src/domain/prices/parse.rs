//! Parsing the two disk tiers of the price table, row by row. PURE: the text
//! comes in, the rows and the refusals go out.

use std::collections::BTreeMap;

use super::builtin::SYNTHETIC;
use super::{strip_date_suffix, ParsedTier, Price, Provenance, RejectedRow};

/// The only schema marker `fetched.json` is read under. A document carrying
/// anything else is ENTIRELY inert — never a row read under a schema we do not
/// recognise (precedent: `hash_algo: "semantic-v1"`, `library_store.rs:259-269`).
pub const FETCHED_SCHEMA: &str = "prices-v1";

/// Validate one row's `(input, output)` numbers. Applied to BOTH disk tiers: a
/// `NaN` would poison `usd` *and* serialise to JSON `null` for a frontend that
/// types it `number`, and a negative price would silently deflate a total.
/// **Refuse, never clamp** — `CONTEXT.md:813` forbids silent self-repair, and
/// `PUT /settings` 400s a cap `< 1` rather than clamping it.
pub fn validate_price(input: f64, output: f64) -> Result<Price, String> {
    for (label, v) in [("input", input), ("output", output)] {
        if !v.is_finite() {
            return Err(format!("{label} price is not a finite number"));
        }
        if v < 0.0 {
            return Err(format!("{label} price is negative"));
        }
    }
    Ok(Price { input, output })
}

/// Reject a family key that could never price anything. Shared by both tiers'
/// key checks that are *not* about de-dating.
fn reject_sentinel_key(key: &str) -> Option<String> {
    key.starts_with('<').then(|| {
        format!(
            "`{key}` looks like a Claude Code sentinel (`{SYNTHETIC}` is priced $0 above every \
             tier, so such a row could only ever be inert)"
        )
    })
}

/// Parse the manual tier (`models.yaml`), row by row. PURE.
///
/// Shape — a MAP, not a list of records, so key uniqueness is structural and the
/// file reads as what it is, a sparse patch:
///
/// ```yaml
/// models:
///   claude-opus-4-8: { input: 4.5, output: 22.5 }
/// ```
///
/// An unknown *field* stays ignored (no `deny_unknown_fields` anywhere in this
/// crate).
pub fn parse_manual(text: &str) -> ParsedTier {
    #[derive(serde::Deserialize)]
    struct Doc {
        #[serde(default)]
        models: BTreeMap<String, serde_yaml::Value>,
    }

    // A blank/comment-only file deserialises to YAML null, which is not a `Doc`.
    // That is an empty patch, not a broken one.
    if text.trim().is_empty() {
        return ParsedTier::default();
    }
    let doc: Doc = match serde_yaml::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return ParsedTier {
                unparseable: Some(e.to_string()),
                ..Default::default()
            }
        }
    };

    let mut tier = ParsedTier::default();
    for (key, value) in doc.models {
        // A dated key would NEVER price anything, and the symptom is
        // indistinguishable from absence. Refuse, do not normalise: stripping
        // would silently collapse two rows the author wanted distinct.
        let family = strip_date_suffix(&key);
        if family != key {
            tier.rejected.push(RejectedRow {
                key: key.clone(),
                why: format!(
                    "keys are FAMILY keys and carry no date suffix — write `{family}` instead"
                ),
            });
            continue;
        }
        if let Some(why) = reject_sentinel_key(&key) {
            tier.rejected.push(RejectedRow { key, why });
            continue;
        }
        // A missing or wrongly-typed field lands here too.
        let num = |field: &str| -> Result<f64, String> {
            value
                .get(field)
                .ok_or_else(|| format!("missing `{field}`"))?
                .as_f64()
                .ok_or_else(|| format!("`{field}` is not a number"))
        };
        match num("input").and_then(|i| num("output").and_then(|o| validate_price(i, o))) {
            Ok(price) => {
                tier.rows.insert(key, price);
            }
            Err(why) => tier.rejected.push(RejectedRow { key, why }),
        }
    }
    tier
}

/// Parse the fetched tier (`fetched.json`). Requires `schema == "prices-v1"`;
/// anything else leaves the tier ENTIRELY inert. PURE.
pub fn parse_fetched(text: &str) -> ParsedTier {
    #[derive(serde::Deserialize)]
    struct Doc {
        schema: Option<String>,
        source: Option<String>,
        fetched_at: Option<String>,
        #[serde(default)]
        models: BTreeMap<String, Price>,
    }

    if text.trim().is_empty() {
        return ParsedTier {
            unparseable: Some("file is empty".to_string()),
            ..Default::default()
        };
    }
    let doc: Doc = match serde_json::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return ParsedTier {
                unparseable: Some(e.to_string()),
                ..Default::default()
            }
        }
    };
    match doc.schema.as_deref() {
        Some(FETCHED_SCHEMA) => {}
        other => {
            return ParsedTier {
                unparseable: Some(format!(
                    "unrecognised schema {} — expected `{FETCHED_SCHEMA}`; no row is read under a \
                     schema this build does not know",
                    other.map_or("(absent)".to_string(), |s| format!("`{s}`"))
                )),
                ..Default::default()
            }
        }
    }

    let mut tier = ParsedTier {
        provenance: match (doc.source, doc.fetched_at) {
            (Some(source), Some(fetched_at)) => Some(Provenance { source, fetched_at }),
            _ => None,
        },
        ..Default::default()
    };
    // The daemon validated these at write time, so rows are taken as written —
    // except for the numeric guard, which is free and defends a hand-edited file
    // (the name says the daemon owns it, but nothing enforces that).
    for (key, price) in doc.models {
        if let Some(why) = reject_sentinel_key(&key) {
            tier.rejected.push(RejectedRow { key, why });
            continue;
        }
        match validate_price(price.input, price.output) {
            Ok(p) => {
                tier.rows.insert(key, p);
            }
            Err(why) => tier.rejected.push(RejectedRow { key, why }),
        }
    }
    tier
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::prices::{fetched_doc, p};

    // --- parse_manual ---

    #[test]
    fn parse_manual_accepts_a_valid_map() {
        let tier = parse_manual(
            "models:\n  claude-opus-4-8: { input: 4.5, output: 22.5 }\n  claude-mythos-5: { input: 10.0, output: 50.0 }\n",
        );
        assert!(tier.unparseable.is_none());
        assert!(tier.rejected.is_empty());
        assert_eq!(tier.rows.get("claude-opus-4-8"), Some(&p(4.5, 22.5)));
        assert_eq!(tier.rows.get("claude-mythos-5"), Some(&p(10.0, 50.0)));
    }

    #[test]
    fn parse_manual_refuses_a_dated_key_and_prints_the_correct_form() {
        let tier =
            parse_manual("models:\n  claude-opus-5-20260501: { input: 5.0, output: 25.0 }\n");
        assert!(tier.rows.is_empty(), "a dated key must not price anything");
        assert_eq!(tier.rejected.len(), 1);
        assert_eq!(tier.rejected[0].key, "claude-opus-5-20260501");
        assert!(
            tier.rejected[0].why.contains("claude-opus-5"),
            "the refusal must print the correct undated form, got: {}",
            tier.rejected[0].why
        );
        // And it must print the FAMILY form, not merely echo the dated key.
        assert!(!tier.rejected[0].why.contains("`claude-opus-5-20260501`"));
    }

    #[test]
    fn parse_manual_refuses_a_sentinel_key() {
        let tier = parse_manual("models:\n  \"<synthetic>\": { input: 99.0, output: 99.0 }\n");
        assert!(tier.rows.is_empty());
        assert_eq!(tier.rejected.len(), 1);
        assert_eq!(tier.rejected[0].key, SYNTHETIC);
    }

    #[test]
    fn parse_manual_refuses_negative_and_non_finite_prices() {
        let neg = parse_manual("models:\n  claude-opus-5: { input: -1.0, output: 25.0 }\n");
        assert!(neg.rows.is_empty());
        assert!(neg.rejected[0].why.contains("negative"));

        let nan = parse_manual("models:\n  claude-opus-5: { input: 5.0, output: .nan }\n");
        assert!(
            nan.rows.is_empty(),
            "a NaN would poison `usd` and serialise as JSON null"
        );
        assert!(nan.rejected[0].why.contains("finite"));

        let inf = parse_manual("models:\n  claude-opus-5: { input: .inf, output: 25.0 }\n");
        assert!(inf.rows.is_empty());
    }

    #[test]
    fn parse_manual_refuses_a_row_with_a_missing_field() {
        let tier = parse_manual("models:\n  claude-opus-5: { input: 5.0 }\n");
        assert!(tier.rows.is_empty());
        assert!(tier.rejected[0].why.contains("output"));
    }

    #[test]
    fn parse_manual_ignores_an_unknown_field_rather_than_rejecting() {
        // No `deny_unknown_fields` anywhere in this crate (ADR-0015 #471).
        let tier = parse_manual(
            "models:\n  claude-opus-5: { input: 5.0, output: 25.0, note: \"discount\" }\n",
        );
        assert!(tier.rejected.is_empty());
        assert_eq!(tier.rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
    }

    #[test]
    fn parse_manual_reports_broken_yaml_without_rows() {
        let tier = parse_manual("models:\n  - this is: [not, a, map\n");
        assert!(tier.unparseable.is_some());
        assert!(tier.rows.is_empty());
    }

    #[test]
    fn parse_manual_treats_an_empty_or_comment_only_file_as_an_empty_patch() {
        // A user who creates the file from the path shown in Settings and has not
        // written a row yet must NOT see a diagnostic: that would read as a defect.
        for text in ["", "   \n", "# nothing yet\n", "models:\n", "models: {}\n"] {
            let tier = parse_manual(text);
            assert!(
                tier.unparseable.is_none(),
                "text = {text:?} → {:?}",
                tier.unparseable
            );
            assert!(tier.rows.is_empty());
            assert!(tier.rejected.is_empty());
        }
    }

    #[test]
    fn parse_manual_keeps_the_good_rows_when_one_is_refused() {
        let tier = parse_manual(
            "models:\n  claude-opus-4-8: { input: 4.5, output: 22.5 }\n  claude-opus-5-20260501: { input: 5.0, output: 25.0 }\n",
        );
        assert_eq!(tier.rows.len(), 1, "one bad row must not void the file");
        assert_eq!(tier.rejected.len(), 1);
    }

    // --- parse_fetched ---

    #[test]
    fn parse_fetched_accepts_the_known_schema_with_its_provenance() {
        let tier = parse_fetched(&fetched_doc(FETCHED_SCHEMA));
        assert!(tier.unparseable.is_none());
        assert_eq!(tier.rows.get("claude-opus-5"), Some(&p(5.0, 25.0)));
        assert_eq!(
            tier.provenance.as_ref().map(|x| x.fetched_at.as_str()),
            Some("2026-07-30T14:12:03Z")
        );
    }

    #[test]
    fn parse_fetched_is_entirely_inert_under_an_unknown_or_absent_schema() {
        for text in [
            fetched_doc("prices-v99"),
            r#"{"models":{"claude-opus-5":{"input":5.0,"output":25.0}}}"#.to_string(),
        ] {
            let tier = parse_fetched(&text);
            assert!(
                tier.rows.is_empty(),
                "no row may be read under a schema this build does not know"
            );
            assert!(tier.unparseable.is_some());
        }
    }

    #[test]
    fn parse_fetched_is_inert_on_broken_or_empty_json() {
        for text in ["{not json", ""] {
            let tier = parse_fetched(text);
            assert!(tier.rows.is_empty());
            assert!(tier.unparseable.is_some());
        }
    }

    #[test]
    fn parse_fetched_still_guards_the_numbers_of_a_hand_edited_file() {
        let text = r#"{"schema":"prices-v1","source":"s","fetched_at":"t","models":{"claude-opus-5":{"input":-5.0,"output":25.0}}}"#;
        let tier = parse_fetched(text);
        assert!(tier.rows.is_empty());
        assert_eq!(tier.rejected.len(), 1);
    }
}
