//! The workspace context's instance default, as `GET /settings` discloses it
//! (ADR-0015): the folder a repository given by its URL is cloned in
//! (CORE-97). The stored tier is handed in by the settings context, which
//! keeps it.

use std::path::{Path, PathBuf};

use pdo_kernel::instance_default::Disclosed;

use crate::domain::clone::{expand_home, DEFAULT_CLONES_FOLDER};

/// The clones folder: `stored → ~/pdo/repos`, as typed (`~` kept). No
/// environment tier.
pub type ClonesFolderSetting = Disclosed<String, String, String>;

pub fn clones_folder(stored: Option<&str>) -> ClonesFolderSetting {
    let stored = stored.map(str::trim).filter(|s| !s.is_empty());
    Disclosed::new(
        stored.unwrap_or(DEFAULT_CLONES_FOLDER).to_string(),
        stored.map(str::to_string),
        None,
        DEFAULT_CLONES_FOLDER.to_string(),
    )
}

/// The clones folder on disk, `~` standing for `home`; why not when it needs
/// a home and the daemon has none.
pub fn clones_folder_on_disk(stored: Option<&str>, home: Option<&Path>) -> Result<PathBuf, String> {
    let folder = clones_folder(stored).effective;
    expand_home(&folder, home)
        .ok_or_else(|| format!("the clones folder {folder} needs a home, and the daemon has none"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_kernel::instance_default::Source;

    #[test]
    fn the_default_is_under_the_home_and_a_stored_folder_wins() {
        let default = clones_folder(None);
        assert_eq!(default.effective, "~/pdo/repos");
        assert_eq!(default.source, Source::Default);
        let stored = clones_folder(Some("/srv/clones"));
        assert_eq!(stored.effective, "/srv/clones");
        assert_eq!(stored.source, Source::Stored);
        assert_eq!(
            clones_folder_on_disk(None, Some(Path::new("/home/me"))).unwrap(),
            PathBuf::from("/home/me/pdo/repos")
        );
        assert!(clones_folder_on_disk(None, None).is_err());
        assert_eq!(
            clones_folder_on_disk(Some("/srv/clones"), None).unwrap(),
            PathBuf::from("/srv/clones")
        );
    }
}
