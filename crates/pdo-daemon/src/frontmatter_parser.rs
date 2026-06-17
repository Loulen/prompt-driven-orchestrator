use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FrontmatterSchema {
    pub fields: HashMap<String, FieldSchema>,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct FieldSchema {
    pub field_type: FieldType,
    pub allowed_values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub enum FieldType {
    String,
    Int,
    Float,
    Bool,
    Enum,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum FrontmatterError {
    #[error("malformed YAML frontmatter: {0}")]
    MalformedYaml(String),
    #[error("field '{field}' has value '{value}' not in allowed values: {allowed:?}")]
    #[allow(dead_code)]
    EnumViolation {
        field: String,
        value: String,
        allowed: Vec<String>,
    },
}

pub fn parse_frontmatter(
    content: &str,
) -> Result<HashMap<String, serde_yaml::Value>, FrontmatterError> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return Ok(HashMap::new());
    }

    let after_first = &trimmed[3..];
    let end_pos = after_first
        .find("\n---")
        .or_else(|| after_first.find("\r\n---"));

    let yaml_block = match end_pos {
        Some(pos) => &after_first[..pos],
        None => return Ok(HashMap::new()),
    };

    let yaml_block = yaml_block.trim();
    if yaml_block.is_empty() {
        return Ok(HashMap::new());
    }

    let value: serde_yaml::Value = serde_yaml::from_str(yaml_block)
        .map_err(|e| FrontmatterError::MalformedYaml(e.to_string()))?;

    let Some(mapping) = value.as_mapping() else {
        return Ok(HashMap::new());
    };

    let mut fields = HashMap::new();
    for (key, val) in mapping {
        if let Some(k) = key.as_str() {
            fields.insert(k.to_string(), val.clone());
        }
    }

    Ok(fields)
}

pub fn parse_frontmatter_from_file(
    path: &Path,
) -> Result<HashMap<String, serde_yaml::Value>, FrontmatterError> {
    match std::fs::read_to_string(path) {
        Ok(content) => parse_frontmatter(&content),
        Err(_) => Ok(HashMap::new()),
    }
}

#[allow(dead_code)]
pub fn validate_against_schema(
    fields: &HashMap<String, serde_yaml::Value>,
    schema: &FrontmatterSchema,
) -> Result<(), FrontmatterError> {
    for (field_name, field_schema) in &schema.fields {
        if let Some(value) = fields.get(field_name) {
            if let Some(ref allowed) = field_schema.allowed_values {
                let val_str = match value {
                    serde_yaml::Value::String(s) => s.clone(),
                    other => serde_yaml::to_string(other)
                        .unwrap_or_default()
                        .trim()
                        .to_string(),
                };
                if !allowed.contains(&val_str) {
                    return Err(FrontmatterError::EnumViolation {
                        field: field_name.clone(),
                        value: val_str,
                        allowed: allowed.clone(),
                    });
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // --- well-formed frontmatter ---

    #[test]
    fn parses_simple_frontmatter() {
        let content = "---\nverdict: PASS\nscore: 8\n---\n\n## Body\nSome content here.";
        let fields = parse_frontmatter(content).unwrap();
        assert_eq!(fields["verdict"], serde_yaml::Value::String("PASS".into()));
        assert_eq!(
            fields["score"],
            serde_yaml::Value::Number(serde_yaml::Number::from(8))
        );
    }

    #[test]
    fn parses_frontmatter_with_bool() {
        let content = "---\napproved: true\n---\n\nBody";
        let fields = parse_frontmatter(content).unwrap();
        assert_eq!(fields["approved"], serde_yaml::Value::Bool(true));
    }

    #[test]
    fn parses_frontmatter_with_float() {
        let content = "---\nconfidence: 0.95\n---\n\nBody";
        let fields = parse_frontmatter(content).unwrap();
        let val = fields["confidence"].as_f64().unwrap();
        assert!((val - 0.95).abs() < 1e-10);
    }

    // --- missing frontmatter (treated as empty) ---

    #[test]
    fn no_frontmatter_returns_empty() {
        let content = "# Just a heading\n\nSome body text.";
        let fields = parse_frontmatter(content).unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn empty_string_returns_empty() {
        let fields = parse_frontmatter("").unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn frontmatter_delimiter_without_closing_returns_empty() {
        let content = "---\nverdict: PASS\n\nBody without closing delimiter";
        let fields = parse_frontmatter(content).unwrap();
        assert!(fields.is_empty());
    }

    #[test]
    fn empty_frontmatter_block_returns_empty() {
        let content = "---\n---\n\nBody";
        let fields = parse_frontmatter(content).unwrap();
        assert!(fields.is_empty());
    }

    // --- malformed YAML ---

    #[test]
    fn malformed_yaml_returns_error() {
        let content = "---\n{{invalid: yaml:::\n---\n\nBody";
        let err = parse_frontmatter(content).unwrap_err();
        assert!(matches!(err, FrontmatterError::MalformedYaml(_)));
    }

    // --- enum validation ---

    #[test]
    fn validates_enum_field_pass() {
        let mut fields = HashMap::new();
        fields.insert("verdict".into(), serde_yaml::Value::String("PASS".into()));

        let schema = FrontmatterSchema {
            fields: [(
                "verdict".into(),
                FieldSchema {
                    field_type: FieldType::Enum,
                    allowed_values: Some(vec!["PASS".into(), "FAIL".into()]),
                },
            )]
            .into_iter()
            .collect(),
        };

        assert!(validate_against_schema(&fields, &schema).is_ok());
    }

    #[test]
    fn validates_enum_field_rejects_invalid() {
        let mut fields = HashMap::new();
        fields.insert("verdict".into(), serde_yaml::Value::String("MAYBE".into()));

        let schema = FrontmatterSchema {
            fields: [(
                "verdict".into(),
                FieldSchema {
                    field_type: FieldType::Enum,
                    allowed_values: Some(vec!["PASS".into(), "FAIL".into()]),
                },
            )]
            .into_iter()
            .collect(),
        };

        let err = validate_against_schema(&fields, &schema).unwrap_err();
        assert!(
            matches!(err, FrontmatterError::EnumViolation { field, value, .. } if field == "verdict" && value == "MAYBE")
        );
    }

    #[test]
    fn validates_missing_field_does_not_error() {
        let fields = HashMap::new();
        let schema = FrontmatterSchema {
            fields: [(
                "verdict".into(),
                FieldSchema {
                    field_type: FieldType::Enum,
                    allowed_values: Some(vec!["PASS".into(), "FAIL".into()]),
                },
            )]
            .into_iter()
            .collect(),
        };

        assert!(validate_against_schema(&fields, &schema).is_ok());
    }

    #[test]
    fn validates_field_without_enum_constraint_always_passes() {
        let mut fields = HashMap::new();
        fields.insert(
            "comment".into(),
            serde_yaml::Value::String("anything goes".into()),
        );

        let schema = FrontmatterSchema {
            fields: [(
                "comment".into(),
                FieldSchema {
                    field_type: FieldType::String,
                    allowed_values: None,
                },
            )]
            .into_iter()
            .collect(),
        };

        assert!(validate_against_schema(&fields, &schema).is_ok());
    }

    // --- parse_frontmatter_from_file ---

    #[test]
    fn nonexistent_file_returns_empty() {
        let fields =
            parse_frontmatter_from_file(Path::new("/nonexistent/path/to/file.md")).unwrap();
        assert!(fields.is_empty());
    }

    // --- frontmatter with leading whitespace ---

    #[test]
    fn frontmatter_with_leading_whitespace() {
        let content = "  ---\nverdict: FAIL\n---\n\nBody";
        let fields = parse_frontmatter(content).unwrap();
        assert_eq!(fields["verdict"], serde_yaml::Value::String("FAIL".into()));
    }

    // --- multiple fields ---

    #[test]
    fn parses_multiple_fields() {
        let content =
            "---\nverdict: PASS\nscore: 9\nreviewer: alice\n---\n\n## Review\nLooks good.";
        let fields = parse_frontmatter(content).unwrap();
        assert_eq!(fields.len(), 3);
        assert_eq!(fields["verdict"], serde_yaml::Value::String("PASS".into()));
        assert_eq!(
            fields["score"],
            serde_yaml::Value::Number(serde_yaml::Number::from(9))
        );
        assert_eq!(
            fields["reviewer"],
            serde_yaml::Value::String("alice".into())
        );
    }
}
