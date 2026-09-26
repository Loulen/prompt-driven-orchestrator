//! The user-declared descriptor file, `<home>/.pdo/harnesses/descriptors.yaml`
//! (ADR-0045): read, never written, never seeded.

use std::path::{Path, PathBuf};

use crate::application::load_registry::load_registry;
use crate::application::ports::{DescriptorRead, DescriptorSource};
use crate::domain::registry::HarnessRegistry;

/// The descriptor file under an injected home root (never `$HOME` itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptorFile {
    path: PathBuf,
}

impl DescriptorFile {
    pub fn under(home_root: &Path) -> Self {
        Self {
            path: HarnessRegistry::descriptors_path(home_root),
        }
    }
}

impl DescriptorSource for DescriptorFile {
    fn read(&self) -> DescriptorRead {
        DescriptorRead {
            path: self.path.clone(),
            text: std::fs::read(&self.path)
                .ok()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
        }
    }
}

/// The effective registry of the home root `home_root`: the floor merged with
/// its descriptor file, by name.
pub fn load(home_root: &Path) -> HarnessRegistry {
    load_registry(&DescriptorFile::under(home_root))
}

/// [`load`] under `home_root` when there is one, the floor alone otherwise
/// (`HOME` unset).
pub fn load_or_builtin(home_root: Option<&Path>) -> HarnessRegistry {
    match home_root {
        Some(home) => load(home),
        None => HarnessRegistry::builtin(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::registry::{claude, opencode, HarnessSource, CLAUDE, OPENCODE};

    #[test]
    fn load_on_a_root_without_a_descriptor_file_is_the_floor_and_silent() {
        let home = tempfile::tempdir().unwrap();
        let reg = load(home.path());
        assert!(reg.resolve(CLAUDE).is_some());
        assert!(reg.resolve(OPENCODE).is_some());
        assert!(reg.diagnostic().is_none());
        assert!(!HarnessRegistry::descriptors_path(home.path()).exists());
    }

    #[test]
    fn load_merges_a_custom_harness_by_name_over_the_floor() {
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "harnesses:\n  my-harness:\n    binary: my-harness\n    launch: [\"exec\", \"my-harness\", \"--auto\"]\n",
        )
        .unwrap();

        let reg = load(home.path());
        let d = reg
            .resolve("my-harness")
            .expect("the custom harness resolves");
        assert_eq!(d.binary, "my-harness");
        assert!(reg.resolve(CLAUDE).is_some());
        assert!(reg.resolve(OPENCODE).is_some());
        assert!(reg.names().contains(&"my-harness".to_string()));
    }

    #[test]
    fn a_broken_descriptor_file_leaves_claude_untouched_and_is_diagnosed() {
        // FP: corrupt the file → it becomes inert and diagnosed; `claude` and
        // `opencode` keep working (the whole disk tier falls through to the floor).
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "harnesses:\n  - not: [a, map\n").unwrap();

        let reg = load(home.path());
        assert_eq!(reg.resolve(CLAUDE), Some(claude()));
        assert_eq!(reg.resolve(OPENCODE), Some(opencode()));
        let d = reg.diagnostic().expect("a broken file must be said");
        assert!(
            d.contains("descriptors.yaml"),
            "the path must be named: {d}"
        );
        assert!(d.contains("inert"));
    }

    #[test]
    fn a_refused_row_falls_through_to_the_floor_and_is_named() {
        // A malformed `claude:` override must not un-define claude: the row is
        // refused, its key falls to the embedded floor, and the refusal is named.
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            // No `binary` → refused whole.
            "harnesses:\n  claude:\n    launch: [\"exec\", \"totally-wrong\"]\n",
        )
        .unwrap();

        let reg = load(home.path());
        assert_eq!(
            reg.resolve(CLAUDE),
            Some(claude()),
            "claude stays the floor"
        );
        assert_eq!(reg.rejected().len(), 1);
        assert_eq!(reg.rejected()[0].name, "claude");
        let d = reg.diagnostic().unwrap();
        assert!(d.contains("`claude`") && d.contains("falling through"));
    }

    #[test]
    fn listing_tags_the_floor_builtin_and_a_disk_harness_descriptor() {
        // #586: the picker's two sections. A novel disk harness is `Descriptor`;
        // the embedded floor stays `Builtin`. The probe binary rides along so the
        // caller can decide `installed` without this module reading `$PATH`.
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "harnesses:\n  my-runner:\n    binary: my-runner-bin\n    launch: [\"exec\", \"my-runner-bin\"]\n",
        )
        .unwrap();

        let reg = load(home.path());
        let listing = reg.listing();

        let claude = listing.iter().find(|h| h.name == CLAUDE).unwrap();
        assert_eq!(claude.source, HarnessSource::Builtin);
        assert_eq!(claude.binary, "claude");

        let opencode = listing.iter().find(|h| h.name == OPENCODE).unwrap();
        assert_eq!(opencode.source, HarnessSource::Builtin);

        let custom = listing.iter().find(|h| h.name == "my-runner").unwrap();
        assert_eq!(custom.source, HarnessSource::Descriptor);
        assert_eq!(
            custom.binary, "my-runner-bin",
            "the probe binary is exposed by name"
        );

        assert_eq!(HarnessSource::Builtin.as_str(), "builtin");
        assert_eq!(HarnessSource::Descriptor.as_str(), "descriptor");
    }

    #[test]
    fn listing_keeps_a_disk_override_of_a_floor_name_builtin() {
        // A disk `claude:` override still lists under "Built-in" (name-based), so
        // the picker's Built-in section always carries claude/opencode — what the
        // design shows — while the override's own binary is what is probed.
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "harnesses:\n  claude:\n    binary: my-claude\n    launch: [\"exec\", \"my-claude\"]\n",
        )
        .unwrap();

        let reg = load(home.path());
        let claude = reg
            .listing()
            .into_iter()
            .find(|h| h.name == CLAUDE)
            .unwrap();
        assert_eq!(claude.source, HarnessSource::Builtin);
        assert_eq!(
            claude.binary, "my-claude",
            "the override's binary is probed"
        );
    }

    #[test]
    fn load_never_writes_to_disk() {
        // The read is pure: loading, even with a healthy file, seeds nothing beyond
        // the file the user already wrote (no fetched.json analogue, no baseline).
        let home = tempfile::tempdir().unwrap();
        let dir = HarnessRegistry::descriptors_path(home.path())
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            HarnessRegistry::descriptors_path(home.path()),
            "harnesses:\n  x:\n    binary: x\n    launch: [\"exec\", \"x\"]\n",
        )
        .unwrap();
        let before: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        let _ = load(home.path());
        let after: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(before, after, "load must not write any file");
    }
}
