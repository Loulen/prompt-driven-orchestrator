//! The pipeline context's adapters: the HTTP routes and `pdo pipeline` (driving), and the
//! registry, the library, the watcher, the live stream, the skill bank and
//! the validations over SQLite (driven).

pub mod cli;
pub mod draft_doc;
pub mod draft_follow;
pub mod events;
pub mod frontmatter_file;
pub mod fs_migration;
pub mod http;
pub mod library_fs;
pub mod registry;
pub mod self_writes;
pub mod skill_bank;
pub mod sqlite_drafts;
pub mod sqlite_validations;
pub mod sqlite_versions;
pub mod watcher;
