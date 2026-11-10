//! The skill context's bank behind the [`SkillBank`] port.

use std::sync::Arc;

use pdo_kernel::Actor;
use pdo_skill::domain::error::SkillError;
use pdo_skill::domain::selection::SkillRef;
use pdo_skill::domain::sidecar::{SidecarImportReport, SidecarSkill};
use pdo_skill::Skills;

use crate::application::ports::{BoxFuture, SkillBank};

/// The bank, through the skill context's own use cases.
pub struct BankOfSkills(pub Arc<Skills>);

impl SkillBank for BankOfSkills {
    fn sidecar_of(&self, references: &[SkillRef]) -> (Vec<SidecarSkill>, Vec<String>) {
        self.0.sidecar_of(references)
    }

    fn import_sidecar<'a>(
        &'a self,
        actor: &'a Actor,
        pipeline_name: &'a str,
        references: Vec<SkillRef>,
        sidecar: &'a [SidecarSkill],
    ) -> BoxFuture<'a, Result<SidecarImportReport, SkillError>> {
        Box::pin(
            self.0
                .import_sidecar(actor, pipeline_name, references, sidecar),
        )
    }
}
