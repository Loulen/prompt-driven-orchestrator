//! Why a pipeline use case did not do what it was asked. The HTTP adapter
//! words each case the way its route always has.

use pdo_kernel::authorization::Denied;
use pdo_skill::domain::error::SkillError;

use crate::domain::pipeline::ParseError;

#[derive(Debug)]
pub enum PipelineError {
    /// An extension refused the Operation.
    Denied(Denied),
    /// No pipeline file under that id.
    NotFound,
    /// The registry's directory cannot be determined.
    NoRegistry,
    /// The pipeline (on disk or in the request) does not parse.
    Invalid(ParseError),
    /// The request is wrong, in the words given.
    BadRequest(String),
    /// The name is blank.
    NameRequired,
    /// Another pipeline owns the target stem or the target name.
    NameTaken(String),
    /// A pipeline of that name already exists (creation).
    AlreadyExists,
    /// Runs not yet completed use the pipeline.
    ActiveRuns(usize),
    /// The document has no top-level `name:` line to rename.
    NoNameLine,
    /// The skill context refused or failed the import of the skills.
    Skills(SkillError),
    /// A write, move or delete failed, in the filesystem's words.
    Storage(String),
}

impl From<Denied> for PipelineError {
    fn from(denied: Denied) -> Self {
        Self::Denied(denied)
    }
}

impl From<ParseError> for PipelineError {
    fn from(error: ParseError) -> Self {
        Self::Invalid(error)
    }
}
