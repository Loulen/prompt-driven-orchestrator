//! The skills the extensions bring into the Banque de skills.

use std::panic::{catch_unwind, AssertUnwindSafe};

use pdo_extension_api::{ExtensionSkill, Manifest};

use super::Extensions;

/// The skills one extension brings into the Banque de skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvidedSkills {
    pub manifest: Manifest,
    pub skills: Vec<ExtensionSkill>,
}

impl Extensions {
    /// Each extension's skills, in registration order.
    pub(super) fn provided_skills(&self) -> Vec<ProvidedSkills> {
        self.registry
            .registered
            .iter()
            .filter_map(
                // An empty list is kept: the seed removes what the extension
                // provided before. A panic is no list at all: it removes nothing.
                |r| match catch_unwind(AssertUnwindSafe(|| r.extension.skills())) {
                    Ok(skills) => Some(ProvidedSkills {
                        manifest: r.manifest.clone(),
                        skills,
                    }),
                    Err(_) => {
                        tracing::error!(
                            "extension `{}` panicked in skills; it brings none",
                            r.manifest.id
                        );
                        None
                    }
                },
            )
            .collect()
    }
}
