//! **Absorption** in Stats (#888, ADR-0077): one Stats row — the **absorbent** —
//! counts the data of other rows of the same level, its **absorbed** members, in
//! every tab that has that dimension. "Combine" in the UI.
//!
//! Three pieces, one module:
//!
//! - **The store** — a table of `pdo.db`, instance configuration beside
//!   `instance_config` (ADR-0015). One row per member: `(dimension, scope,
//!   member)` is unique, so a member has exactly one absorbent. The storage is
//!   **flat, one level**: [`combine`] flattens on write (absorbing an absorbent
//!   moves its members over), so the read never resolves anything transitively.
//! - **The resolver** — [`AbsorptionResolver`], loaded once per Stats request
//!   and applied where each fold reads a Run's identity, BEFORE aggregating. The
//!   event log is never rewritten: `uncombined=true` is just a read with an empty
//!   resolver, and removing a member takes effect at once, history included.
//! - **The Run identity read** — [`RunIdentity`]: the `pipeline_id` →
//!   `pipeline_name` → `(unknown)` fallback and the frozen `node_defs`, read in
//!   ONE place instead of being copied into Sessions, Cost and Performance. It is
//!   where the resolver substitutes an absorbed key with its absorbent's.
//!
//! #890 ships the `pipeline` dimension; #891 adds the rename-born absorption
//! ([`record_rename`], origin `rename`); #892 adds the two other dimensions:
//!
//! - **Nodes**, scoped to the key of their absorbent Pipeline (or of a lone
//!   Pipeline). A Node absorption applies to every Run counted under that
//!   Pipeline row — in every tree where the Node shows. Taking a Pipeline out
//!   of its absorption takes its Nodes out of the Node absorptions of that
//!   scope ([`remove_member`]).
//! - **Models**, by verbatim id (ADR-0065): they fold the « By model » axis
//!   of Cost and Performance and, since #906 (ADR-0078 amending ADR-0077), the
//!   model × effort couples of every Node too; their couples meet by effort.
//!
//! #906 adds two more (ADR-0078), both with a **frozen scope**: an absorption
//! is written once per RAW key it covered when it was posed, and read on the
//! raw key — no carry, no cascade:
//!
//! - **Efforts** of one model, scoped to a raw model id. Posed on a model row,
//!   it is written for the row's key and for every model it absorbed then.
//! - **Couples** (model × effort) of one Node, scoped to a raw `(Pipeline,
//!   Node)` pair ([`couple_scope`]). Local to the Node row: never on the
//!   « By model » axis. Its keys are couples as the global absorptions resolve
//!   them, and they go through that resolution again at read time.
//!
//! A couple of a Node resolves in this order ([`RunIdentity::couple`]): (1) the
//! effort, on the raw model; (2) the model; (3) the couples meet by effort;
//! (4) the couple, on the raw Node. The « By model » axis stops at step 3.

pub mod identity;
pub mod list;
pub mod request;
pub mod resolver;
pub mod scopes;
pub mod tallies;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The three dimensions an absorption can have.
pub const PIPELINE: &str = "pipeline";
pub const NODE: &str = "node";
pub const MODEL: &str = "model";
pub const EFFORT: &str = "effort";
pub const COUPLE: &str = "couple";

/// The key of an effort: the effort itself, `""` for « not set ».
pub fn effort_key(effort: Option<&str>) -> String {
    effort.unwrap_or("").to_string()
}

pub fn effort_of_key(key: &str) -> Option<String> {
    (!key.is_empty()).then(|| key.to_string())
}

/// The name of an effort, as its row shows it.
pub fn effort_name(effort: Option<&str>) -> String {
    effort.unwrap_or("not set").to_string()
}

/// The key of a model × effort couple: `model|effort` (`model|` for « not
/// set »). A model id never carries a `|`.
pub fn couple_key(model: &str, effort: Option<&str>) -> String {
    format!("{model}|{}", effort.unwrap_or(""))
}

/// `model|effort` → `(model, effort)`; `None` on a key that is no couple.
pub fn split_couple_key(key: &str) -> Option<(String, Option<String>)> {
    let (model, effort) = key.rsplit_once('|')?;
    (!model.is_empty()).then(|| (model.to_string(), effort_of_key(effort)))
}

/// The name of a couple, as its row shows it: `model · effort`.
pub fn couple_name(model: &str, effort: Option<&str>) -> String {
    format!("{model} · {}", effort_name(effort))
}

/// The scope of a couple absorption: one `(Pipeline, Node)` pair, as a JSON
/// array — unambiguous whatever the two keys hold.
pub fn couple_scope(pipeline: &str, node: &str) -> String {
    serde_json::json!([pipeline, node]).to_string()
}

pub fn parse_couple_scope(scope: &str) -> Option<(String, String)> {
    let pair: (String, String) = serde_json::from_str(scope).ok()?;
    (!pair.0.is_empty() && !pair.1.is_empty()).then_some(pair)
}

/// Where an absorption came from: the operator's Combine (`manual`) or a
/// Pipeline rename that changed its id (`rename`, #891).
pub const ORIGIN_MANUAL: &str = "manual";
pub const ORIGIN_RENAME: &str = "rename";

/// The Pipeline key of a Run whose `run_started` names neither an id nor a name.
pub const UNKNOWN_PIPELINE: &str = "(unknown)";

/// One stored member row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsorptionRow {
    pub dimension: String,
    pub scope: String,
    pub scope_name: String,
    pub member: String,
    pub member_name: String,
    pub absorbent: String,
    pub absorbent_name: String,
    pub origin: String,
    pub created_at: String,
}

/// A key and the name to show for it. The UI only ever shows the name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "StatsNamed")]
pub struct Named {
    pub key: String,
    pub name: String,
}

/// Where an absorption lives: its dimension, and — for a Node absorption — the
/// key and name of the Pipeline row it is scoped to (empty otherwise).
#[derive(Debug, Clone, Copy)]
pub struct Scope<'a> {
    pub dimension: &'a str,
    pub key: &'a str,
    pub name: &'a str,
}

#[cfg(test)]
mod tests {
    use super::identity::pipeline_identity;
    use super::resolver::AbsorptionResolver;
    use super::*;

    #[test]
    fn an_absorbed_run_takes_the_absorbents_stored_name() {
        let rows = vec![AbsorptionRow {
            dimension: PIPELINE.to_string(),
            scope: String::new(),
            scope_name: String::new(),
            member: "old".to_string(),
            member_name: "Old".to_string(),
            absorbent: "new".to_string(),
            absorbent_name: "New".to_string(),
            origin: ORIGIN_MANUAL.to_string(),
            created_at: "2026-09-24T00:00:00Z".to_string(),
        }];
        let resolver = AbsorptionResolver::from_rows(&rows);
        let absorbed = resolver
            .run_identity(&serde_json::json!({"pipeline_id": "old", "pipeline_name": "Old"}));
        assert_eq!(absorbed.pipeline_key, "new");
        assert_eq!(absorbed.pipeline_name, "New");
        assert!(!absorbed.own_name);
        let own = resolver
            .run_identity(&serde_json::json!({"pipeline_id": "new", "pipeline_name": "New v2"}));
        // An absorbed Run seen first names the row; the absorbent's own Run wins.
        let mut slot = String::new();
        absorbed.adopt_name(&mut slot);
        assert_eq!(slot, "New");
        own.adopt_name(&mut slot);
        assert_eq!(slot, "New v2");
        absorbed.adopt_name(&mut slot);
        assert_eq!(slot, "New v2");
    }

    #[test]
    fn the_identity_falls_back_from_id_to_name_to_unknown() {
        assert_eq!(
            pipeline_identity(&serde_json::json!({"pipeline_name": "Only name"})),
            ("Only name".to_string(), "Only name".to_string())
        );
        assert_eq!(
            pipeline_identity(&serde_json::json!({})),
            (UNKNOWN_PIPELINE.to_string(), UNKNOWN_PIPELINE.to_string())
        );
    }

    #[test]
    fn couple_keys_and_scopes_round_trip() {
        assert_eq!(couple_key("m", None), "m|");
        assert_eq!(
            split_couple_key("m|high"),
            Some(("m".to_string(), Some("high".to_string())))
        );
        assert_eq!(split_couple_key("m|"), Some(("m".to_string(), None)));
        assert_eq!(split_couple_key("nope"), None);
        assert_eq!(couple_name("m", None), "m · not set");
        let scope = couple_scope("a \"b\"", "n");
        assert_eq!(
            parse_couple_scope(&scope),
            Some(("a \"b\"".to_string(), "n".to_string()))
        );
    }
}
