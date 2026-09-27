//! The pipeline context's adapters: the HTTP routes (driving), and the
//! registry, the library, the watcher, the live stream and the skill bank
//! (driven).

pub mod events;
pub mod frontmatter_file;
pub mod fs_migration;
pub mod http;
pub mod library_fs;
pub mod registry;
pub mod self_writes;
pub mod skill_bank;
pub mod watcher;
