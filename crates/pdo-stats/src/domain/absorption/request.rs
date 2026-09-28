//! A Combine as the operator asks for it, and the rules that refuse one before
//! anything is written: a dimension Stats does not combine, a scope that does
//! not fit the dimension, a row with no key, an absorption with no member.

use serde::Deserialize;

use super::{split_couple_key, AbsorptionRow, Named, COUPLE, EFFORT, MODEL, NODE, PIPELINE};

/// One row of a Combine request. `scope` names the Pipeline row a Node was
/// selected under (#892): the daemon refuses Nodes of two different Pipelines
/// rather than trust the request's single `scope`.
#[derive(Debug, Clone, Deserialize)]
pub struct CombineRow {
    pub key: String,
    pub name: String,
    #[serde(default)]
    pub scope: Option<String>,
}

impl CombineRow {
    pub fn named(&self) -> Named {
        Named {
            key: self.key.clone(),
            name: self.name.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CombineRequest {
    pub dimension: String,
    /// The key of the Pipeline row a Node absorption is scoped to; empty on the
    /// other dimensions.
    #[serde(default)]
    pub scope: String,
    /// That Pipeline's name, for Settings.
    #[serde(default)]
    pub scope_name: String,
    pub absorbent: CombineRow,
    pub members: Vec<CombineRow>,
}

/// The refusal for a dimension Stats does not combine, if any.
pub fn unsupported_dimension(dimension: &str) -> Option<String> {
    (![PIPELINE, NODE, MODEL, EFFORT, COUPLE].contains(&dimension))
        .then(|| format!("unsupported absorption dimension: {dimension}"))
}

/// The absorbent `member` is counted under in `dimension` × `scope`, if any.
fn absorbent_of<'a>(
    rows: &'a [AbsorptionRow],
    dimension: &str,
    scope: &str,
    member: &str,
) -> Option<&'a str> {
    rows.iter()
        .find(|row| row.dimension == dimension && row.scope == scope && row.member == member)
        .map(|row| row.absorbent.as_str())
}

/// The refusal for a scope that does not fit the dimension, if any: a Node
/// absorption lives under ONE Pipeline row — one that is not itself counted
/// under another — and the other dimensions have no scope. `rows` are the
/// absorptions stored right now.
pub fn invalid_scope(body: &CombineRequest, rows: &[AbsorptionRow]) -> Option<String> {
    match body.dimension.as_str() {
        EFFORT => return invalid_effort_scope(body, rows),
        COUPLE => return invalid_couple_scope(body, rows),
        _ => {}
    }
    if body.dimension != NODE {
        return (!body.scope.is_empty())
            .then(|| format!("a {} absorption has no scope", body.dimension));
    }
    if body.scope.trim().is_empty() {
        return Some("a node absorption needs the pipeline its nodes run under".to_string());
    }
    if !rows_share_the_scope(body) {
        return Some("nodes of different pipelines cannot be combined".to_string());
    }
    absorbent_of(rows, PIPELINE, "", &body.scope)
        .map(|_| "this pipeline is combined into another: combine its nodes there".to_string())
}

/// Whether every row of the request was selected under the request's scope.
fn rows_share_the_scope(body: &CombineRequest) -> bool {
    std::iter::once(&body.absorbent)
        .chain(body.members.iter())
        .filter_map(|row| row.scope.as_deref())
        .all(|scope| scope == body.scope)
}

/// An effort absorption lives in ONE model row (grilling Q1): efforts of two
/// models are refused, and so is a model row counted under another.
fn invalid_effort_scope(body: &CombineRequest, rows: &[AbsorptionRow]) -> Option<String> {
    if body.scope.trim().is_empty() {
        return Some("an effort absorption needs the model its efforts ran on".to_string());
    }
    if !rows_share_the_scope(body) {
        return Some("efforts of different models cannot be combined".to_string());
    }
    absorbent_of(rows, MODEL, "", &body.scope)
        .map(|_| "this model is combined into another: combine its efforts there".to_string())
}

/// A couple absorption lives in ONE Node row (grilling Q4): couples of two
/// Nodes are refused, and so is a Node row counted under another.
fn invalid_couple_scope(body: &CombineRequest, rows: &[AbsorptionRow]) -> Option<String> {
    let Some((pipeline, node)) = super::parse_couple_scope(&body.scope) else {
        return Some(
            "a couple absorption needs the pipeline and the node its couples ran under".to_string(),
        );
    };
    if !rows_share_the_scope(body) {
        return Some("couples of different nodes cannot be combined".to_string());
    }
    if let Some(row) = std::iter::once(&body.absorbent)
        .chain(body.members.iter())
        .find(|row| split_couple_key(&row.key).is_none())
    {
        return Some(format!("not a model × effort couple: {}", row.name));
    }
    if absorbent_of(rows, PIPELINE, "", &pipeline).is_some() {
        return Some(
            "this pipeline is combined into another: combine its couples there".to_string(),
        );
    }
    absorbent_of(rows, NODE, &pipeline, &node)
        .map(|_| "this node is combined into another: combine its couples there".to_string())
}

/// The absorbent and the members to write, or the refusal: a row with no key
/// (« not set » is the effort key `""`, so an effort row may be keyed by it),
/// or no member besides the absorbent.
pub fn absorbent_and_members(body: &CombineRequest) -> Result<(Named, Vec<Named>), String> {
    let empty_allowed = body.dimension == EFFORT;
    if !empty_allowed && body.absorbent.key.trim().is_empty() {
        return Err("absorbent key is empty".to_string());
    }
    let absorbent = body.absorbent.named();
    let members: Vec<Named> = body
        .members
        .iter()
        .filter(|m| m.key != absorbent.key)
        .map(CombineRow::named)
        .collect();
    if members.is_empty() {
        return Err("an absorption needs at least one other member".to_string());
    }
    if !empty_allowed && members.iter().any(|m| m.key.trim().is_empty()) {
        return Err("member key is empty".to_string());
    }
    Ok((absorbent, members))
}

/// Whether an absorption of `dimension` is written with a frozen scope
/// (ADR-0078): once per raw key it covers when it is posed.
pub fn frozen_scope(dimension: &str) -> bool {
    [EFFORT, COUPLE].contains(&dimension)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(key: &str, scope: Option<&str>) -> CombineRow {
        CombineRow {
            key: key.into(),
            name: key.into(),
            scope: scope.map(str::to_string),
        }
    }

    fn request(dimension: &str, scope: &str, rows: &[CombineRow]) -> CombineRequest {
        CombineRequest {
            dimension: dimension.into(),
            scope: scope.into(),
            scope_name: String::new(),
            absorbent: rows[0].clone(),
            members: rows[1..].to_vec(),
        }
    }

    fn stored(dimension: &str, scope: &str, member: &str) -> AbsorptionRow {
        AbsorptionRow {
            dimension: dimension.into(),
            scope: scope.into(),
            scope_name: String::new(),
            member: member.into(),
            member_name: member.into(),
            absorbent: "other".into(),
            absorbent_name: "other".into(),
            origin: "manual".into(),
            created_at: String::new(),
        }
    }

    #[test]
    fn a_node_absorption_lives_under_one_uncombined_pipeline() {
        let nodes = [row("a", Some("p")), row("b", Some("p"))];
        assert_eq!(invalid_scope(&request(NODE, "p", &nodes), &[]), None);
        let mixed = [row("a", Some("p")), row("b", Some("q"))];
        assert_eq!(
            invalid_scope(&request(NODE, "p", &mixed), &[]).as_deref(),
            Some("nodes of different pipelines cannot be combined")
        );
        assert_eq!(
            invalid_scope(&request(NODE, "p", &nodes), &[stored(PIPELINE, "", "p")]).as_deref(),
            Some("this pipeline is combined into another: combine its nodes there")
        );
        assert_eq!(
            invalid_scope(&request(PIPELINE, "p", &nodes), &[]).as_deref(),
            Some("a pipeline absorption has no scope")
        );
    }

    #[test]
    fn a_couple_absorption_names_its_node_and_couples_only() {
        let scope = super::super::couple_scope("p", "n");
        let couples = [row("m|high", None), row("m|", None)];
        assert_eq!(invalid_scope(&request(COUPLE, &scope, &couples), &[]), None);
        let not_couple = [row("m|high", None), row("plain", None)];
        assert_eq!(
            invalid_scope(&request(COUPLE, &scope, &not_couple), &[]).as_deref(),
            Some("not a model × effort couple: plain")
        );
        assert_eq!(
            invalid_scope(
                &request(COUPLE, &scope, &couples),
                &[stored(NODE, "p", "n")]
            )
            .as_deref(),
            Some("this node is combined into another: combine its couples there")
        );
    }

    #[test]
    fn an_effort_row_may_be_keyed_by_not_set_but_a_pipeline_may_not() {
        let efforts = [row("", None), row("high", None)];
        assert!(absorbent_and_members(&request(EFFORT, "m", &efforts)).is_ok());
        assert_eq!(
            absorbent_and_members(&request(PIPELINE, "", &efforts)).unwrap_err(),
            "absorbent key is empty"
        );
        let alone = [row("a", None), row("a", None)];
        assert_eq!(
            absorbent_and_members(&request(PIPELINE, "", &alone)).unwrap_err(),
            "an absorption needs at least one other member"
        );
        assert_eq!(
            unsupported_dimension("harness").as_deref(),
            Some("unsupported absorption dimension: harness")
        );
    }
}
