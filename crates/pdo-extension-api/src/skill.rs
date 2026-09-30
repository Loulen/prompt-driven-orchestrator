/// A skill an extension brings with it: its `SKILL.md` and its reference
/// files, embedded in the extension (usually with `include_str!` /
/// `include_bytes!`).
///
/// When the daemon starts, the core seeds it into the Banque de skills like
/// its own skills: locked against every edit, marked as provided by the
/// extension, rewritten when the extension's version or the content changes.
/// A Run that selects it gets it in its worktree, like any skill of the bank
/// (ADR-0062).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ExtensionSkill {
    /// The skill's name: the `name` of its `SKILL.md` frontmatter, and the
    /// folder it is delivered into (`.agents/skills/<name>/`).
    pub name: String,
    /// The `SKILL.md`, frontmatter included.
    pub skill_md: String,
    /// The other files of the skill's folder, in any order.
    pub files: Vec<SkillFile>,
}

impl ExtensionSkill {
    pub fn new(name: impl Into<String>, skill_md: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            skill_md: skill_md.into(),
            files: Vec::new(),
        }
    }

    /// The same skill, with one more reference file at `path`
    /// (`/`-separated, relative to the skill's folder).
    pub fn with_file(mut self, path: impl Into<String>, content: impl Into<Vec<u8>>) -> Self {
        self.files.push(SkillFile {
            path: path.into(),
            content: content.into(),
        });
        self
    }
}

/// One reference file of an [`ExtensionSkill`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SkillFile {
    /// `/`-separated, relative to the skill's folder; never `SKILL.md`.
    pub path: String,
    pub content: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_skill_carries_its_text_and_its_files() {
        let skill = ExtensionSkill::new("demo", "---\nname: demo\n---\n")
            .with_file("references/cli.md", "# CLI");
        assert_eq!(skill.name, "demo");
        assert_eq!(skill.files[0].path, "references/cli.md");
        assert_eq!(skill.files[0].content, b"# CLI");
    }
}
