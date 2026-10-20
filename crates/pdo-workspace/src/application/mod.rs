//! The workspace use cases. Every mutation builds an Opération and asks the
//! Autorisation d'opération before touching anything. The reads of the
//! Projets and the browse of the disk ask it too ([`read_access`], CORE-5);
//! the other reads go straight to the ports.

mod add_project_member;
mod create_project;
mod create_repository;
mod delete_project;
mod error;
mod fast_forward_branch;
mod fetch_remotes;
pub mod ports;
mod queries;
pub mod read_access;
mod remove_project_member;
mod save_provisioning_rules;
mod update_project;
mod workspace;

#[cfg(test)]
pub(crate) mod testing;

pub use error::{WorkspaceError, WorkspaceResult};
pub use fast_forward_branch::FastForwarded;
pub use queries::{PreviewRequest, RECENT_REPOSITORIES};
pub use remove_project_member::MemberRemoved;
pub use save_provisioning_rules::{RulesOwner, INSTANCE_SCOPE_ID};
pub use update_project::ProjectUpdate;
pub use workspace::{Workspace, WorkspacePorts};
