//! The paths of a skill's reference files (#671): always inside the skill's
//! folder, never outside it.

use super::error::SkillError;
use super::skill::SKILL_MD;

/// Normalise a reference-file path the client sent: `/`-separated, relative,
/// made only of plain components (no `.`, no `..`, no empty segment, no
/// backslash, no NUL). This is the **one** gate keeping a write inside the
/// skill's folder (#671 AC "les chemins sortant du dossier du skill sont
/// refusés (400)"); every file endpoint goes through it.
pub fn normalise_file_path(raw: &str) -> Result<String, SkillError> {
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.contains('\\')
        || trimmed.contains('\0')
    {
        return Err(SkillError::InvalidPath(raw.to_string()));
    }
    let mut parts = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(SkillError::InvalidPath(raw.to_string()));
        }
        parts.push(segment);
    }
    Ok(parts.join("/"))
}

/// A normalised reference-file path. Refuses `SKILL.md` itself unless
/// `allow_skill_md` (only the text editor reads and writes it).
pub fn reference_file(raw: &str, allow_skill_md: bool) -> Result<String, SkillError> {
    let rel = normalise_file_path(raw)?;
    if rel == SKILL_MD && !allow_skill_md {
        return Err(SkillError::SkillMdReserved);
    }
    Ok(rel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_paths_leaving_the_skill_folder_are_refused() {
        for bad in [
            "",
            "/etc/passwd",
            "../x",
            "a/../b",
            "a//b",
            "./a",
            "a\\b",
            "a/./b",
        ] {
            assert!(
                matches!(normalise_file_path(bad), Err(SkillError::InvalidPath(_))),
                "{bad:?} should be refused"
            );
        }
        assert_eq!(
            normalise_file_path(" examples/login.spec.ts ").unwrap(),
            "examples/login.spec.ts"
        );
        assert_eq!(normalise_file_path("notes.md").unwrap(), "notes.md");
    }

    #[test]
    fn skill_md_is_reserved_unless_the_editor_asks() {
        assert_eq!(
            reference_file("SKILL.md", false).unwrap_err(),
            SkillError::SkillMdReserved
        );
        assert_eq!(reference_file("SKILL.md", true).unwrap(), "SKILL.md");
    }
}
