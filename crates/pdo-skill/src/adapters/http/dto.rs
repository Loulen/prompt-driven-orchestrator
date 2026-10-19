//! The request bodies of the skill routes.

use serde::Deserialize;

use super::deserialize_double_option;
use crate::domain::import::{ImportFolder, ImportItem};
use crate::domain::update::UpdateItem;

#[derive(Deserialize)]
pub(super) struct CreateSkillRequest {
    /// The pasted `SKILL.md` text, frontmatter included.
    pub content: String,
    /// The bank label; defaults to the frontmatter `name`.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub folder_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct UpdateSkillRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub folder_id: Option<Option<String>>,
}

#[derive(Deserialize)]
pub(super) struct CreateSkillFolderRequest {
    pub name: String,
    #[serde(default)]
    pub parent_id: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct UpdateSkillFolderRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_double_option")]
    pub parent_id: Option<Option<String>>,
}

#[derive(Deserialize)]
pub(super) struct UploadFromPathRequest {
    /// Absolute path on the daemon's host, picked in the explorer.
    pub from_path: String,
    /// Destination relative to the skill folder; defaults to the file name.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct ScanSkillSourceRequest {
    /// Client-chosen id; the clone is kept under it for the import that follows
    /// and `POST …/scan/{id}/cancel` kills it.
    pub scan_id: String,
    pub source: String,
}

#[derive(Deserialize)]
pub(super) struct ImportSkillsRequest {
    pub scan_id: String,
    pub source: String,
    #[serde(default)]
    pub folder: Option<ImportFolder>,
    pub items: Vec<ImportItem>,
}

#[derive(Deserialize)]
pub(super) struct RescanSkillFolderRequest {
    pub scan_id: String,
}

#[derive(Deserialize)]
pub(super) struct UpdateSkillFolderFromSourceRequest {
    pub scan_id: String,
    pub items: Vec<UpdateItem>,
}

/// `POST /settings/skills/export`: the skills, and the folders whose skills
/// (sub-folders included) the zip carries.
#[derive(Deserialize)]
pub(super) struct ExportSkillsRequest {
    #[serde(default)]
    pub skill_ids: Vec<String>,
    #[serde(default)]
    pub folder_ids: Vec<String>,
}

/// `POST /settings/skills/upload?name=`: the uploaded file's name, which names
/// the folder the new skills go to.
#[derive(Deserialize)]
pub(super) struct UploadSkillsQuery {
    #[serde(default)]
    pub name: Option<String>,
}
