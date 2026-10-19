//! Import the skills sidecar of a portable pipeline document into the bank
//! (#673): see [`crate::domain::sidecar`] for the policy.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};

use super::skills::Skills;
use crate::domain::error::SkillError;
use crate::domain::selection::SkillRef;
use crate::domain::sidecar::{
    import_folder_name, label_candidates, RenamedSkill, SidecarImportReport, SidecarSkill,
};
use crate::domain::skill::{SkillFolder, SKILL_MD};
use crate::domain::skill_md::validate_skill_md;

impl Skills {
    /// Apply the import policy to the bank for the `references` of the
    /// pipeline `pipeline_name`. The pipeline relabels its references with
    /// [`SidecarImportReport::final_names`] (the id never changes).
    pub async fn import_sidecar(
        &self,
        actor: &Actor,
        pipeline_name: &str,
        references: Vec<SkillRef>,
        sidecar: &[SidecarSkill],
    ) -> Result<SidecarImportReport, SkillError> {
        if references.is_empty() {
            return Ok(SidecarImportReport::default());
        }
        self.authorize_import(actor)?;
        self.import_carried(&import_folder_name(pipeline_name), references, sidecar)
            .await
    }

    /// The Operation every import into the bank asks.
    pub(crate) fn authorize_import(&self, actor: &Actor) -> Result<(), SkillError> {
        self.authorize(
            actor,
            Operation::new(ResourceKind::Skill, Verb::Action("import".into())),
        )?;
        Ok(())
    }

    /// The import policy of [`crate::domain::sidecar`] for `references`,
    /// filing the created skills under the root folder `folder_name`. The
    /// caller has asked the Operation.
    pub(crate) async fn import_carried(
        &self,
        folder_name: &str,
        references: Vec<SkillRef>,
        sidecar: &[SidecarSkill],
    ) -> Result<SidecarImportReport, SkillError> {
        let mut report = SidecarImportReport::default();
        for reference in references {
            let label = Some(reference.name.trim().to_string()).filter(|l| !l.is_empty());
            if let Some(existing) = self.skills.get(&reference.id).await? {
                report.kept.push(SkillRef {
                    id: existing.id,
                    name: existing.name,
                });
                continue;
            }
            let Some(carried) = sidecar.iter().find(|s| s.id == reference.id) else {
                report.warnings.push(format!(
                    "skills.{}: skill `{}` is absent from the bank and from the sidecar — the \
                     pipeline keeps the reference and its node shows a warning until the skill \
                     is added",
                    reference.id,
                    label.clone().unwrap_or_else(|| reference.id.clone())
                ));
                report.missing.push(reference);
                continue;
            };
            let Some(Ok(content)) = carried
                .skill_md()
                .map(|bytes| String::from_utf8(bytes.to_vec()))
            else {
                report.warnings.push(format!(
                    "skills.{}: the sidecar's SKILL.md is not UTF-8 text — skill not imported",
                    reference.id
                ));
                report.missing.push(reference);
                continue;
            };
            let parsed = match validate_skill_md(&content) {
                Ok(parsed) => parsed,
                Err(error) => {
                    report.warnings.push(format!(
                        "skills.{}: the sidecar's SKILL.md is invalid ({error}) — skill not imported",
                        reference.id
                    ));
                    report.missing.push(reference);
                    continue;
                }
            };
            let wanted = label.clone().unwrap_or_else(|| parsed.name.clone());
            let name = self.free_label(&wanted).await?;
            let folder = match &report.folder {
                Some(folder) => folder.clone(),
                None => {
                    let folder = self.import_folder(folder_name).await?;
                    report.folder = Some(folder.clone());
                    folder
                }
            };
            let created = self
                .index_new(Some(&reference.id), &content, Some(&name), Some(&folder.id))
                .await?;
            for (path, data) in &carried.files {
                if path == SKILL_MD {
                    continue;
                }
                if let Err(error) = self.write_reference_file(&created.id, path, data) {
                    report.warnings.push(format!(
                        "skills.{}: reference file `{path}` not imported: {error}",
                        created.id
                    ));
                }
            }
            if name != wanted {
                report.warnings.push(format!(
                    "skills.{}: the name `{wanted}` is already used by another skill — imported as \
                     `{name}`",
                    created.id
                ));
                report.renamed.push(RenamedSkill {
                    id: created.id.clone(),
                    from: wanted,
                    to: name.clone(),
                });
            }
            report.created.push(SkillRef {
                id: created.id,
                name: created.name,
            });
        }
        Ok(report)
    }

    /// The root folder created skills are filed under, reused when it exists.
    async fn import_folder(&self, name: &str) -> Result<SkillFolder, SkillError> {
        if let Some(existing) = self
            .folders
            .list()
            .await?
            .into_iter()
            .find(|f| f.parent_id.is_none() && f.name.eq_ignore_ascii_case(name))
        {
            return Ok(existing);
        }
        self.new_folder(name, None).await
    }

    /// A label the bank does not use yet: `name`, then `name-2`, `name-3`…
    async fn free_label(&self, wanted: &str) -> Result<String, SkillError> {
        for candidate in label_candidates(wanted) {
            if self.skills.find_by_name(&candidate).await?.is_none() {
                return Ok(candidate);
            }
        }
        Err(SkillError::Storage(format!(
            "could not find a free label for `{wanted}`"
        )))
    }
}
