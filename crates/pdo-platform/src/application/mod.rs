//! The use cases of the platform context, one per file, and the ports they need
//! (`ports.rs`). Every mutation builds an Operation and goes through the
//! Autorisation d'opération before it touches anything.

pub mod apply_update;
pub mod check_for_update;
pub mod get_changelog;
pub mod get_update_status;
pub mod install_service;
pub mod list_audit;
pub mod mount_page;
pub mod page_journal;
pub mod ports;
pub mod read_attempt_log;
pub mod read_pages;
pub mod record_audit;
pub mod release_run_pages;
pub mod service_status;
pub mod uninstall_service;
pub mod unmount_page;
pub mod update_context;
