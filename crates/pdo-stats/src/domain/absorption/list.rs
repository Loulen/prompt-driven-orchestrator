//! The absorptions as `GET /stats/absorptions` lists them.

use std::collections::BTreeMap;

use serde::Serialize;
use ts_rs::TS;

use super::{AbsorptionRow, Named};

/// One absorption as `GET /stats/absorptions` lists it: the absorbent and its
/// members, names beside keys (the UI shows the names only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "StatsAbsorption")]
pub struct AbsorptionView {
    pub dimension: String,
    pub scope: String,
    /// A Node absorption's Pipeline, by name (#892) — empty on the others.
    #[serde(skip_serializing_if = "String::is_empty")]
    #[ts(optional = nullable)]
    pub scope_name: String,
    pub absorbent: Named,
    pub members: Vec<AbsorptionMemberView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "StatsAbsorptionMember")]
pub struct AbsorptionMemberView {
    pub key: String,
    pub name: String,
    pub origin: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "StatsAbsorptionList")]
pub struct AbsorptionList {
    pub absorptions: Vec<AbsorptionView>,
}

pub fn group(rows: Vec<AbsorptionRow>) -> AbsorptionList {
    let mut grouped: BTreeMap<(String, String, String), AbsorptionView> = BTreeMap::new();
    for row in rows {
        let view = grouped
            .entry((
                row.dimension.clone(),
                row.scope.clone(),
                row.absorbent.clone(),
            ))
            .or_insert_with(|| AbsorptionView {
                dimension: row.dimension.clone(),
                scope: row.scope.clone(),
                scope_name: row.scope_name.clone(),
                absorbent: Named {
                    key: row.absorbent.clone(),
                    name: row.absorbent_name.clone(),
                },
                members: Vec::new(),
            });
        view.members.push(AbsorptionMemberView {
            key: row.member,
            name: row.member_name,
            origin: row.origin,
            created_at: row.created_at,
        });
    }
    AbsorptionList {
        absorptions: grouped.into_values().collect(),
    }
}
