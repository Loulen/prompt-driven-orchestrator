//! The type of a frontmatter field (`type:` under a port's `frontmatter`): the
//! spellings the engine knows, the aliases a hand-written YAML may use, and the
//! lint that names a type nobody knows.

use serde::{Deserialize, Deserializer};

use super::definition::PipelineDef;

/// The field types the editor, the edge conditions and the output check know.
pub const FIELD_TYPES: &[&str] = &["enum", "int", "string", "bool", "list"];

/// The canonical spelling of a field type: the JSON-schema names `boolean`,
/// `integer` and `array` are aliases of `bool`, `int` and `list`. Any other
/// spelling comes back unchanged (the lint names it).
pub fn canonical_field_type(raw: &str) -> String {
    match raw.trim() {
        "boolean" => "bool",
        "integer" => "int",
        "array" => "list",
        other => other,
    }
    .to_string()
}

pub(super) fn deserialize_field_type<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    String::deserialize(d).map(|raw| canonical_field_type(&raw))
}

/// One warning per frontmatter field whose type is not one of [`FIELD_TYPES`],
/// on an output port or a live artifact.
pub(super) fn unknown_field_types(pipeline: &PipelineDef) -> Vec<String> {
    let mut warnings = Vec::new();
    for node in &pipeline.nodes {
        let schemas = node
            .outputs
            .iter()
            .map(|p| (&p.name, &p.frontmatter))
            .chain(
                node.live_artifacts
                    .iter()
                    .map(|a| (&a.name, &a.frontmatter)),
            );
        for (owner, schema) in schemas {
            let Some(schema) = schema else { continue };
            let mut fields: Vec<_> = schema.iter().collect();
            fields.sort_by(|a, b| a.0.cmp(b.0));
            for (field, decl) in fields {
                if !FIELD_TYPES.contains(&decl.field_type.as_str()) {
                    warnings.push(format!(
                        "node '{}': output '{}' field '{}' has unknown type '{}' \
                         (expected one of: {})",
                        node.id,
                        owner,
                        field,
                        decl.field_type,
                        FIELD_TYPES.join(", ")
                    ));
                }
            }
        }
    }
    warnings
}

#[cfg(test)]
mod tests {
    use super::super::parse_pipeline;

    fn pipeline_with_field_type(ty: &str) -> String {
        format!(
            r#"
name: t
nodes:
  - id: start
    name: Start
    type: start
    outputs: [{{ name: user_prompt }}]
  - id: review
    name: Review
    type: agent
    inputs: [{{ name: in }}]
    outputs:
      - name: out
        frontmatter:
          approved: {{ type: {ty} }}
  - id: end
    name: End
    type: end
    inputs: [{{ name: result }}]
edges: []
"#
        )
    }

    #[test]
    fn boolean_is_an_alias_of_bool() {
        let result = parse_pipeline(&pipeline_with_field_type("boolean")).unwrap();
        let review = result
            .pipeline
            .nodes
            .iter()
            .find(|n| n.id == "review")
            .unwrap();
        let fm = review.outputs[0].frontmatter.as_ref().unwrap();
        assert_eq!(fm["approved"].field_type, "bool");
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.message.contains("unknown type")),
            "{:?}",
            result.diagnostics
        );
    }

    #[test]
    fn integer_and_array_are_aliases_of_int_and_list() {
        for (alias, canonical) in [("integer", "int"), ("array", "list")] {
            let result = parse_pipeline(&pipeline_with_field_type(alias)).unwrap();
            let review = result
                .pipeline
                .nodes
                .iter()
                .find(|n| n.id == "review")
                .unwrap();
            let fm = review.outputs[0].frontmatter.as_ref().unwrap();
            assert_eq!(fm["approved"].field_type, canonical);
        }
    }

    #[test]
    fn an_unknown_field_type_is_linted() {
        let result = parse_pipeline(&pipeline_with_field_type("number")).unwrap();
        let warning = result
            .diagnostics
            .iter()
            .find(|d| d.message.contains("unknown type 'number'"))
            .unwrap_or_else(|| panic!("no lint in {:?}", result.diagnostics));
        assert!(warning.message.contains("node 'review'"));
        assert!(warning.message.contains("field 'approved'"));
    }
}
