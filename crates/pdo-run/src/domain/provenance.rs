//! A Run's Provenance: where a Run an extension created comes from, as a
//! person reads it (`NewRun::provenance`, CONTEXT.md § *Provenance*).

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The Provenance a Run carries: the `label` and the `link` (a deep link of
/// the interface) the extension `extension` gave when it created the Run.
/// Frozen into `RunStarted` and kept by a retry. Distinct from
/// `triggered_by`: a Trigger's Run has none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct RunProvenance {
    pub label: String,
    pub link: String,
    /// The id of the extension that created the Run.
    pub extension: String,
}

impl RunProvenance {
    /// The Provenance `provenance` the extension `extension` declares, as the
    /// core freezes it (its label trimmed), or why the core refuses it.
    pub fn declared(
        extension: &str,
        provenance: pdo_extension_api::Provenance,
    ) -> Result<Self, String> {
        if let Some(refusal) = provenance.refusal() {
            return Err(refusal);
        }
        Ok(Self {
            label: provenance.label.trim().to_string(),
            link: provenance.link,
            extension: extension.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdo_extension_api::Provenance;

    #[test]
    fn a_declared_provenance_is_frozen_with_its_extension() {
        assert_eq!(
            RunProvenance::declared("test", Provenance::new("  Demo ", "#/ext/test/x")),
            Ok(RunProvenance {
                label: "Demo".into(),
                link: "#/ext/test/x".into(),
                extension: "test".into(),
            })
        );
        assert!(RunProvenance::declared("test", Provenance::new("Demo", "https://x")).is_err());
    }
}
