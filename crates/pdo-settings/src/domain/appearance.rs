//! The Apparence of an Actor (CONTEXT.md → *Apparence*, ADR-0096): the
//! document the interface keeps for each Actor, and the shape the daemon
//! checks.
//!
//! The daemon checks the **shape** only — the types, the version, the size —
//! never the list of the elements nor the values of the Theme: a new
//! Élément masquable, a new Theme, need no change here. Every field but
//! `version` is a deviation from the interface's defaults, absent when the
//! Actor kept the default: a default changed later reaches who never touched
//! it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The largest document the daemon keeps, in bytes of its JSON.
pub const MAX_DOCUMENT_BYTES: usize = 32 * 1024;
/// The most deviations a document's `elements` holds.
pub const MAX_ELEMENTS: usize = 512;
/// The longest element id (`<surface>.ext.<extension>:<contribution>`).
pub const MAX_ELEMENT_ID: usize = 200;
/// The longest Theme name.
pub const MAX_THEME: usize = 64;

/// An Actor's Apparence, as the interface writes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields = nullable)]
pub struct AppearanceDocument {
    /// The version of the document's form, for the interface to migrate an
    /// older one. At least 1.
    pub version: u32,
    /// The Theme (`acogni`, `classic`…); absent: the default Theme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// The Mode sombre; absent: the default mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dark_mode: Option<bool>,
    /// Nodes compactes; absent: the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compact_nodes: Option<bool>,
    /// The Palette daltonienne (« Colour-blind friendly status colours »);
    /// absent: the default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour_blind_status: Option<bool>,
    /// The deviations of the Éléments masquables: `element id → visible`.
    /// An element absent here follows its default.
    #[serde(default)]
    pub elements: BTreeMap<String, bool>,
}

/// What `GET` and `PUT /actors/me/appearance` answer: the caller's document,
/// `null` while it has never written one (the interface then adopts the
/// values this browser held).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct ActorAppearance {
    pub appearance: Option<AppearanceDocument>,
}

/// Why a document is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppearanceError {
    /// Larger than [`MAX_DOCUMENT_BYTES`] (`413`).
    TooLarge(usize),
    /// Not the shape of an [`AppearanceDocument`] (`400`).
    Malformed(String),
}

impl std::fmt::Display for AppearanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AppearanceError::TooLarge(size) => write!(
                f,
                "the appearance is {size} bytes, more than the {MAX_DOCUMENT_BYTES} the daemon keeps"
            ),
            AppearanceError::Malformed(why) => write!(f, "invalid appearance: {why}"),
        }
    }
}

impl AppearanceDocument {
    /// Read a document from the bytes of a request, checking its shape.
    pub fn parse(raw: &[u8]) -> Result<Self, AppearanceError> {
        if raw.len() > MAX_DOCUMENT_BYTES {
            return Err(AppearanceError::TooLarge(raw.len()));
        }
        let malformed = |e: serde_json::Error| AppearanceError::Malformed(e.to_string());
        // An object, never an array: serde would read `[1]` as the fields in order.
        let value: serde_json::Value = serde_json::from_slice(raw).map_err(malformed)?;
        if !value.is_object() {
            return Err(AppearanceError::Malformed("not a JSON object".into()));
        }
        let document: Self = serde_json::from_value(value).map_err(malformed)?;
        document.check()?;
        Ok(document)
    }

    fn check(&self) -> Result<(), AppearanceError> {
        let malformed = |why: String| Err(AppearanceError::Malformed(why));
        if self.version == 0 {
            return malformed("`version` starts at 1".into());
        }
        if let Some(theme) = &self.theme {
            if theme.trim().is_empty() || theme.len() > MAX_THEME {
                return malformed(format!(
                    "`theme` must be a name of 1 to {MAX_THEME} characters"
                ));
            }
        }
        if self.elements.len() > MAX_ELEMENTS {
            return malformed(format!(
                "`elements` holds {} entries, at most {MAX_ELEMENTS}",
                self.elements.len()
            ));
        }
        if let Some(id) = self
            .elements
            .keys()
            .find(|id| id.trim().is_empty() || id.len() > MAX_ELEMENT_ID)
        {
            return malformed(format!(
                "the element id `{id}` must be 1 to {MAX_ELEMENT_ID} characters"
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: serde_json::Value) -> Result<AppearanceDocument, AppearanceError> {
        AppearanceDocument::parse(json.to_string().as_bytes())
    }

    #[test]
    fn a_full_document_reads_back_as_written() {
        let json = serde_json::json!({
            "version": 1,
            "theme": "classic",
            "dark_mode": false,
            "compact_nodes": true,
            "colour_blind_status": true,
            "elements": { "runs.list.cost": true, "runs.list.ext.test:mark": false },
        });
        let document = parse(json.clone()).unwrap();
        assert_eq!(document.theme.as_deref(), Some("classic"));
        assert_eq!(document.colour_blind_status, Some(true));
        assert_eq!(document.elements.len(), 2);
        assert_eq!(serde_json::to_value(&document).unwrap(), json);
    }

    #[test]
    fn only_the_version_is_required() {
        let document = parse(serde_json::json!({ "version": 1 })).unwrap();
        assert_eq!(
            document,
            AppearanceDocument {
                version: 1,
                ..Default::default()
            }
        );
        // A default is not written back.
        assert_eq!(
            serde_json::to_value(&document).unwrap(),
            serde_json::json!({ "version": 1, "elements": {} })
        );
    }

    #[test]
    fn an_unknown_theme_or_element_is_kept() {
        let document = parse(serde_json::json!({
            "version": 3, "theme": "tomorrow", "elements": { "nowhere.thing": true },
        }))
        .unwrap();
        assert_eq!(document.theme.as_deref(), Some("tomorrow"));
        assert!(document.elements["nowhere.thing"]);
    }

    #[test]
    fn a_wrong_shape_is_refused() {
        for json in [
            serde_json::json!({}),
            serde_json::json!({ "version": 0 }),
            serde_json::json!({ "version": "1" }),
            serde_json::json!({ "version": 1, "dark_mode": "yes" }),
            serde_json::json!({ "version": 1, "colour_blind_status": "on" }),
            serde_json::json!({ "version": 1, "theme": "" }),
            serde_json::json!({ "version": 1, "theme": "x".repeat(MAX_THEME + 1) }),
            serde_json::json!({ "version": 1, "elements": { "runs.list.cost": "hidden" } }),
            serde_json::json!({ "version": 1, "elements": { "": true } }),
            serde_json::json!({ "version": 1, "elements": [] }),
            serde_json::json!({ "version": 1, "layout": "wide" }),
            serde_json::json!([1]),
        ] {
            assert!(
                matches!(parse(json.clone()), Err(AppearanceError::Malformed(_))),
                "{json}"
            );
        }
        assert!(matches!(
            AppearanceDocument::parse(b"not json"),
            Err(AppearanceError::Malformed(_))
        ));
    }

    #[test]
    fn too_many_elements_or_too_many_bytes_are_refused() {
        let many: serde_json::Map<_, _> = (0..=MAX_ELEMENTS)
            .map(|i| (format!("e.{i}"), serde_json::Value::Bool(true)))
            .collect();
        assert!(matches!(
            parse(serde_json::json!({ "version": 1, "elements": many })),
            Err(AppearanceError::Malformed(_))
        ));
        let big = vec![b' '; MAX_DOCUMENT_BYTES + 1];
        assert_eq!(
            AppearanceDocument::parse(&big),
            Err(AppearanceError::TooLarge(MAX_DOCUMENT_BYTES + 1))
        );
    }
}
