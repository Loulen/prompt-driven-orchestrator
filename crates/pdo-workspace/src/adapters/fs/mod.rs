//! The filesystem side of the workspace: the explorer, and provisioning on disk.

pub mod browse;
pub mod provisioner;

use std::path::Path;

use crate::application::ports::SourceTree;
use crate::domain::provisioning::{ProvisioningPlan, ScopedRules};

pub use browse::FsBrowser;

/// The [`SourceTree`] over the real source repository.
pub struct FsSourceTree;

impl SourceTree for FsSourceTree {
    fn plan(
        &self,
        repository: &Path,
        scoped: &[ScopedRules],
        git_ref: &str,
    ) -> anyhow::Result<ProvisioningPlan> {
        provisioner::resolve_at_git_ref(repository, scoped, git_ref)
    }
}
