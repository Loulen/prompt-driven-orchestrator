//! The one gate to disk: a `SKILL.md` whose frontmatter the harness would
//! ignore (no `name`, no `description`, empty body) is refused with a named
//! reason and **nothing is written** (#668 AC "rien n'est écrit sur disque").

use super::error::SkillError;

/// The parsed, validated content of a `SKILL.md` — what [`validate_skill_md`]
/// returns and what a creation writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSkillMd {
    pub name: String,
    pub description: String,
    /// The whole frontmatter, for the read-only detail table. A `Mapping` (not a
    /// `BTreeMap`) so the table keeps the author's key order.
    pub frontmatter: serde_yaml::Mapping,
    /// The markdown body after the closing `---`, trimmed.
    pub body: String,
}

/// `^[a-z0-9]+(-[a-z0-9]+)*$` without a regex dependency.
pub fn is_kebab_case(name: &str) -> bool {
    if name.is_empty() || name.starts_with('-') || name.ends_with('-') || name.contains("--") {
        return false;
    }
    name.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

fn scalar_string(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        serde_yaml::Value::Number(n) => Some(n.to_string()),
        serde_yaml::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Split `content` into its frontmatter YAML text and its body. `None` when there
/// is no opening `---` at the very top (leading whitespace tolerated).
fn split_frontmatter(content: &str) -> Option<(&str, &str)> {
    let trimmed = content.trim_start();
    let rest = trimmed.strip_prefix("---")?;
    // The opening fence must end its line.
    let rest = rest
        .strip_prefix("\r\n")
        .or_else(|| rest.strip_prefix('\n'))?;
    // Closing fence: a line that is exactly `---`.
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end_matches(['\r', '\n']) == "---" {
            let yaml = &rest[..offset];
            let body = &rest[offset + line.len()..];
            return Some((yaml, body));
        }
        offset += line.len();
    }
    None
}

/// The single gate to disk. Checks, in the order the paste popup lists them:
/// frontmatter block found, `name` present and kebab-case, `description`
/// present, body not empty. Uniqueness is the store's job.
pub fn validate_skill_md(content: &str) -> Result<ParsedSkillMd, SkillError> {
    let (yaml, body) = split_frontmatter(content).ok_or(SkillError::NoFrontmatter)?;
    let frontmatter: serde_yaml::Mapping = if yaml.trim().is_empty() {
        serde_yaml::Mapping::new()
    } else {
        serde_yaml::from_str(yaml).map_err(|e| SkillError::MalformedFrontmatter(e.to_string()))?
    };
    let name = frontmatter
        .get(serde_yaml::Value::from("name"))
        .and_then(scalar_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or(SkillError::MissingName)?;
    if !is_kebab_case(&name) {
        return Err(SkillError::NameNotKebabCase(name));
    }
    let description = frontmatter
        .get(serde_yaml::Value::from("description"))
        .and_then(scalar_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or(SkillError::MissingDescription)?;
    let body = body.trim();
    if body.is_empty() {
        return Err(SkillError::EmptyBody);
    }
    Ok(ParsedSkillMd {
        name,
        description,
        frontmatter,
        body: body.to_string(),
    })
}

/// A label the bank would accept: trimmed, not blank.
pub fn normalise_label(name: &str) -> Result<String, SkillError> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(SkillError::EmptyLabel);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = "---\nname: tdd\ndescription: Test-driven development.\n---\n\n# TDD\n\nRed, green, refactor.\n";

    #[test]
    fn kebab_case_rules() {
        assert!(is_kebab_case("tdd"));
        assert!(is_kebab_case("code-review"));
        assert!(is_kebab_case("a1-b2"));
        assert!(!is_kebab_case("TDD"));
        assert!(!is_kebab_case("code_review"));
        assert!(!is_kebab_case("-lead"));
        assert!(!is_kebab_case("trail-"));
        assert!(!is_kebab_case("double--dash"));
        assert!(!is_kebab_case(""));
        assert!(!is_kebab_case("with space"));
    }

    #[test]
    fn validate_accepts_a_complete_skill_md() {
        let parsed = validate_skill_md(VALID).unwrap();
        assert_eq!(parsed.name, "tdd");
        assert_eq!(parsed.description, "Test-driven development.");
        assert!(parsed.body.starts_with("# TDD"));
        assert!(parsed
            .frontmatter
            .contains_key(serde_yaml::Value::from("name")));
    }

    #[test]
    fn validate_refuses_each_missing_piece_with_a_named_reason() {
        assert_eq!(
            validate_skill_md("# no frontmatter\n\nbody").unwrap_err(),
            SkillError::NoFrontmatter
        );
        assert_eq!(
            validate_skill_md("---\ndescription: x\n---\nbody").unwrap_err(),
            SkillError::MissingName
        );
        assert_eq!(
            validate_skill_md("---\nname: TDD\ndescription: x\n---\nbody").unwrap_err(),
            SkillError::NameNotKebabCase("TDD".into())
        );
        assert_eq!(
            validate_skill_md("---\nname: tdd\n---\nbody").unwrap_err(),
            SkillError::MissingDescription
        );
        assert_eq!(
            validate_skill_md("---\nname: tdd\ndescription: x\n---\n\n  \n").unwrap_err(),
            SkillError::EmptyBody
        );
        assert!(matches!(
            validate_skill_md("---\nname: [\n---\nbody").unwrap_err(),
            SkillError::MalformedFrontmatter(_)
        ));
        // An unclosed fence is "no frontmatter block", not a YAML error.
        assert_eq!(
            validate_skill_md("---\nname: tdd\ndescription: x\nbody").unwrap_err(),
            SkillError::NoFrontmatter
        );
    }

    #[test]
    fn a_label_is_trimmed_and_never_blank() {
        assert_eq!(normalise_label("  tdd ").unwrap(), "tdd");
        assert_eq!(normalise_label("   ").unwrap_err(), SkillError::EmptyLabel);
    }
}
