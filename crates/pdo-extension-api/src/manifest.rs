use serde::Serialize;

use crate::API_VERSION;

/// What an extension says about itself.
///
/// The `id` names the extension's space: the prefix of the attributes it stamps,
/// its routes under `/ext/<id>/`, its data folder. Keep it short, lowercase and
/// stable: lowercase ASCII letters, digits and `-`, starting with a letter or a
/// digit. The daemon refuses to start on an id that breaks this rule, or that
/// two extensions share.
///
/// `api_version` is the version of this API the extension requires. It defaults
/// to the version the extension was compiled against; the daemon refuses to
/// start when it does not implement a compatible one (same major, and same minor
/// while the API is in 0.x, at least as recent).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
}

impl Manifest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            api_version: API_VERSION.to_string(),
        }
    }

    /// The same manifest, requiring version `api_version` of this API.
    pub fn requiring_api(mut self, api_version: impl Into<String>) -> Self {
        self.api_version = api_version.into();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_the_api_it_was_compiled_against_by_default() {
        let manifest = Manifest::new("demo", "Demo", "1.2.3");
        assert_eq!(manifest.api_version, API_VERSION);
        assert_eq!(manifest.requiring_api("0.9.0").api_version, "0.9.0");
    }
}
