use serde::Serialize;

/// A `when` clause: a declarative condition on the context of a location,
/// evaluated by the core before it renders a contribution (a contribution whose
/// clause is false is not rendered at all).
///
/// A key is a dotted path into the location's context: `run`, `run.status`,
/// `project.name`, `run.attributes.<extension>/<key>`, `entity.kind`, `path`…
/// (the reference page lists what each location hands over). An absent key
/// reads as nothing.
///
/// On the wire, an object with one field: `{"has": "run"}`,
/// `{"eq": ["run.status", "running"]}`, `{"not": …}`, `{"all": […]}`,
/// `{"any": […]}`.
///
/// Non-exhaustive: adding an operator is a minor evolution of the API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub enum When {
    /// The key holds something: neither absent, `null`, `false` nor `""`.
    Has(String),
    /// The key's value, as text (`String(value)`), equals the second field.
    Eq(String, String),
    /// The inner clause is false.
    Not(Box<When>),
    /// Every inner clause holds; true when there is none.
    All(Vec<When>),
    /// At least one inner clause holds; false when there is none.
    Any(Vec<When>),
}

impl When {
    pub fn has(key: impl Into<String>) -> Self {
        When::Has(key.into())
    }

    pub fn eq(key: impl Into<String>, value: impl Into<String>) -> Self {
        When::Eq(key.into(), value.into())
    }

    /// The negation of `condition`.
    #[allow(clippy::should_implement_trait)]
    pub fn not(condition: When) -> Self {
        When::Not(Box::new(condition))
    }

    pub fn all(conditions: impl IntoIterator<Item = When>) -> Self {
        When::All(conditions.into_iter().collect())
    }

    pub fn any(conditions: impl IntoIterator<Item = When>) -> Self {
        When::Any(conditions.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_clause_is_an_object_with_one_field_on_the_wire() {
        let clause = When::all([
            When::has("run"),
            When::not(When::eq("run.status", "archived")),
            When::any([When::has("project"), When::eq("run.pipeline", "demo")]),
        ]);
        assert_eq!(
            serde_json::to_value(&clause).unwrap(),
            json!({ "all": [
                { "has": "run" },
                { "not": { "eq": ["run.status", "archived"] } },
                { "any": [{ "has": "project" }, { "eq": ["run.pipeline", "demo"] }] },
            ] })
        );
    }
}
