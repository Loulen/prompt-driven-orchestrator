//! The skills an extension provides (#33, ADR-0081): seeded at every daemon
//! start like PDO's own, into one folder per extension, locked, and marked with
//! the extension they come with.

use super::skill::ProvidedBy;

/// One skill an extension provides, as the seed receives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionSkillSeed {
    /// The extension's id and version.
    pub extension: ProvidedBy,
    /// The extension's display name: the name of its folder in the bank.
    pub extension_name: String,
    /// The skill's name, which its `SKILL.md` frontmatter must carry.
    pub name: String,
    pub skill_md: String,
    /// The reference files, `(path relative to the skill's folder, content)`.
    pub files: Vec<(String, Vec<u8>)>,
}

impl ExtensionSkillSeed {
    /// The skill's id in the bank: stable across versions, so a tier that
    /// selects it keeps selecting it.
    pub fn id(&self) -> String {
        extension_skill_id(&self.extension.id, &self.name)
    }
}

/// The bank id of the skill `name` of the extension `extension`.
pub fn extension_skill_id(extension: &str, name: &str) -> String {
    format!("ext-{extension}-{name}")
}

/// The id of the bank folder holding the skills of the extension `extension`.
pub fn extension_folder_id(extension: &str) -> String {
    format!("skf-ext-{extension}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_derived_from_the_extension_and_the_name() {
        let seed = ExtensionSkillSeed {
            extension: ProvidedBy {
                id: "pm".into(),
                version: "0.3.0".into(),
            },
            extension_name: "Projet".into(),
            name: "pm".into(),
            skill_md: String::new(),
            files: vec![],
        };
        assert_eq!(seed.id(), "ext-pm-pm");
        assert_eq!(extension_folder_id("pm"), "skf-ext-pm");
    }
}
