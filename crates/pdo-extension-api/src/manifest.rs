use serde::Serialize;

use crate::{ResourceDeclaration, API_VERSION};

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
/// start when it does not implement a compatible one: the same major, at least
/// as recent, and while the API is in 0.x, not older than
/// [`OLDEST_COMPATIBLE_API_VERSION`](crate::OLDEST_COMPATIBLE_API_VERSION).
///
/// `resources` are the Ressources the extension owns
/// ([`ResourceKind::Extension`](crate::ResourceKind::Extension)), which the
/// core adds to its catalogue (`GET /resources`, [`Host::catalogue`]) as
/// `<id>/<kind>`. None by default.
///
/// [`Host::catalogue`]: crate::Host::catalogue
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Manifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub api_version: String,
    pub resources: Vec<ResourceDeclaration>,
}

impl Manifest {
    pub fn new(id: impl Into<String>, name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            version: version.into(),
            api_version: API_VERSION.to_string(),
            resources: Vec::new(),
        }
    }

    /// The same manifest, declaring one more Ressource.
    pub fn with_resource(mut self, resource: ResourceDeclaration) -> Self {
        self.resources.push(resource);
        self
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

    #[test]
    fn declares_no_resource_by_default() {
        let manifest = Manifest::new("demo", "Demo", "1.2.3");
        assert!(manifest.resources.is_empty());
        let manifest =
            manifest.with_resource(ResourceDeclaration::new("item", "Items", ["create"]));
        assert_eq!(manifest.resources[0].kind, "item");
    }
}
