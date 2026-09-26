//! The adapters of the platform context: the driving side (HTTP routes, the
//! CLI, the periodic version check) and the implementations of the ports
//! (SQLite, the files behind the mounts, the release source, the update
//! journal, the detached executor, the host).

pub mod cli;
pub mod detached_executor;
pub mod env;
pub mod fs_update_journal;
pub mod github_releases;
pub mod host_installation;
pub mod host_service;
pub mod http;
pub mod page_files;
pub mod service_health_probe;
pub mod sqlite_audit_log;
pub mod sqlite_page_mounts;
pub mod update_schedule;
