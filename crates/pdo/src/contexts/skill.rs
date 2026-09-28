//! The `skill` context: the bank's index over SQLite, its content under the
//! daemon's repo root, the Sources through git, the delivery into a worktree,
//! and who selects a skill (the tiers the settings context reads).

use std::path::Path;
use std::sync::Arc;

use pdo_kernel::extension_host::ExtensionHost;
use pdo_settings::adapters::contexts::SkillTierReferents;
use pdo_skill::adapters::{delivery, fs, source, sqlite};
use pdo_skill::{Skills, SkillsPorts};

/// The skill context over `db` and `repo_root` (where the bank keeps its
/// content), asking `host` before every mutation. The boot's seed wires its own,
/// over a connection of its own.
pub(crate) fn skills(
    db: &sqlx::SqlitePool,
    repo_root: &Path,
    host: Arc<dyn ExtensionHost>,
) -> Arc<Skills> {
    Arc::new(Skills::new(
        SkillsPorts {
            skills: Arc::new(sqlite::SqliteSkillIndex::new(db.clone())),
            folders: Arc::new(sqlite::SqliteFolderIndex::new(db.clone())),
            sources: Arc::new(sqlite::SqliteSourceHistory::new(db.clone())),
            content: Arc::new(fs::FsSkillContent::new(repo_root)),
            scanner: Arc::new(source::GitSourceScanner::new()),
            delivery: Arc::new(delivery::WorktreeDelivery::new(repo_root)),
            referents: Arc::new(SkillTierReferents {
                db: db.clone(),
                repo_root: repo_root.to_path_buf(),
            }),
        },
        host,
    ))
}
