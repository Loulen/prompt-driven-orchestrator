use serde::Serialize;

/// What an extension says about itself.
///
/// The `id` names the extension's space: the prefix of the attributes it stamps,
/// its routes under `/ext/<id>/`, its data folder. Keep it short, lowercase and
/// stable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
}

impl Manifest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
        }
    }
}
