//! Create a pipeline: a Start and an End, named after the request.

use pdo_kernel::{Actor, Operation, ResourceKind, Verb};
use tracing::info;

use super::error::PipelineError;
use super::pipelines::Pipelines;
use super::ports::CreateFailure;
use super::registry_names::listed_name;
use super::Created;
use crate::domain::naming::pipeline_slug;

impl Pipelines {
    /// The file stem is the slug of the name (#886), like duplicate, import
    /// and rename, so the first save never takes the new file for a rename.
    /// Two stems can carry the same visible name, so an existing name is a
    /// collision too, whatever its stem.
    pub fn create(&self, actor: &Actor, name: &str) -> Result<Created, PipelineError> {
        self.authorize(actor, Operation::new(ResourceKind::Pipeline, Verb::Create))?;
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(PipelineError::NameRequired);
        }
        let id = pipeline_slug(&name);
        if self.registry.dir().is_none() {
            return Err(PipelineError::NoRegistry);
        }
        if self
            .registry
            .files()
            .iter()
            .any(|file| listed_name(file).trim() == name)
        {
            return Err(PipelineError::AlreadyExists);
        }
        let path = match self.registry.create(&id, &scaffold(&name, &id)) {
            Ok(path) => path,
            Err(CreateFailure::Exists) => return Err(PipelineError::AlreadyExists),
            Err(CreateFailure::Io(e)) => {
                return Err(PipelineError::Storage(format!("write failed: {e}")))
            }
        };
        info!("Created pipeline {id} at {}", path.display());
        Ok(Created { id, path })
    }
}

/// A new pipeline: a Start, an End, nothing between. The name is quoted
/// exactly when it needs it (`my: name`), so the scaffold parses.
fn scaffold(name: &str, id: &str) -> String {
    let name_scalar = serde_yaml::to_string(&name)
        .map(|scalar| scalar.trim_end().to_string())
        .unwrap_or_else(|_| id.to_string());
    format!(
        "name: {name_scalar}\nversion: \"1.0\"\n\nvariables: {{}}\n\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\n\nedges: []\n"
    )
}
