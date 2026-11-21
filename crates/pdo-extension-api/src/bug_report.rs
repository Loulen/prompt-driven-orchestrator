//! The Destinations de bug report an extension declares (API 0.28), and the
//! Rapport de bug it receives there.
//!
//! The core's own destination, a new issue of its public repository opened in
//! the browser, is always there and first. An extension adds its own,
//! possibly depending on the Actor
//! ([`Extension::bug_report_destinations`](crate::Extension::bug_report_destinations)),
//! and receives the report the person sends to one of them
//! ([`Extension::receive_bug_report`](crate::Extension::receive_bug_report)):
//! it creates the item on its side and answers where it is.

use serde::{Deserialize, Serialize};

/// Where an extension accepts a Rapport de bug, as « Send to » lists it under
/// the extension's name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct BugReportDestination {
    /// Its id, unique within the extension: the core names it
    /// `<extension id>/<id>`.
    pub id: String,
    /// What « Send to » shows, and the primary button after « Send to ».
    pub label: String,
    /// Whether the prompts of the joined Pipeline may come with the report:
    /// « Include prompts » can be checked only then. `false` by default.
    pub accepts_prompts: bool,
    /// The icon of its row: an SVG the extension serves, drawn in the colour
    /// of the text like a page's icon. Without one, the label's first letter.
    pub icon: Option<String>,
}

impl BugReportDestination {
    pub fn new(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            accepts_prompts: false,
            icon: None,
        }
    }

    /// The same destination, accepting the prompts.
    pub fn accepting_prompts(mut self) -> Self {
        self.accepts_prompts = true;
        self
    }

    /// The same destination, with the SVG served at `url` as its icon.
    pub fn with_icon(mut self, url: impl Into<String>) -> Self {
        self.icon = Some(url.into());
        self
    }
}

/// The sections of a Rapport de bug, each as its markdown; a section the
/// report leaves out is `None`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct BugReportFields {
    #[serde(default)]
    pub description: Option<String>,
    /// The version of PDO (`PDO 2.81.0`, or a distribution's with its core).
    #[serde(default)]
    pub version: Option<String>,
    /// The daemon's system.
    #[serde(default)]
    pub os: Option<String>,
    /// The installed harnesses with their versions.
    #[serde(default)]
    pub harnesses: Option<String>,
    /// The details of the joined Run: statuses, failure reasons, durations.
    #[serde(default)]
    pub run: Option<String>,
    /// The structure of the joined Pipeline, without its prompts.
    #[serde(default)]
    pub pipeline: Option<String>,
    /// The prompts of the joined Pipeline, node by node: only toward a
    /// destination that accepts them, and only when the person asked.
    #[serde(default)]
    pub prompts: Option<String>,
}

impl BugReportFields {
    /// The fields of a report holding only `description`.
    pub fn described(description: impl Into<String>) -> Self {
        Self {
            description: Some(description.into()),
            ..Self::default()
        }
    }
}

/// The screenshot a report carries: its bytes and their media type
/// (`image/png`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Screenshot {
    pub media_type: String,
    pub bytes: Vec<u8>,
}

impl Screenshot {
    pub fn new(media_type: impl Into<String>, bytes: impl Into<Vec<u8>>) -> Self {
        Self {
            media_type: media_type.into(),
            bytes: bytes.into(),
        }
    }
}

/// A Rapport de bug sent to one of the extension's destinations.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct BugReport {
    /// The id of the destination, as the extension declared it (without its
    /// `<extension id>/` prefix).
    pub destination: String,
    /// A title: the description's first line, shortened.
    pub title: String,
    /// The whole report as markdown, one `### Heading` per section: the text
    /// the person saw in the Preview (and possibly edited).
    pub text: String,
    /// The same report, section by section.
    pub fields: BugReportFields,
    pub screenshot: Option<Screenshot>,
}

impl BugReport {
    pub fn new(
        destination: impl Into<String>,
        title: impl Into<String>,
        text: impl Into<String>,
        fields: BugReportFields,
    ) -> Self {
        Self {
            destination: destination.into(),
            title: title.into(),
            text: text.into(),
            fields,
            screenshot: None,
        }
    }

    /// The same report, with a screenshot.
    pub fn with_screenshot(mut self, screenshot: Screenshot) -> Self {
        self.screenshot = Some(screenshot);
        self
    }
}

/// What a destination made of a report: the item it created.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct BugReportSent {
    /// How a person names the item (`BUG-12`): the toast says « Bug report
    /// sent: <reference> ».
    pub reference: String,
    /// Where the item is: an absolute URL, or a link inside the interface
    /// (`#/ext/<id>/…`), which the toast's « Open » follows.
    pub link: String,
}

impl BugReportSent {
    pub fn new(reference: impl Into<String>, link: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            link: link.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_refuses_the_prompts_and_has_no_icon_by_default() {
        let inbox = BugReportDestination::new("inbox", "Inbox");
        assert!(!inbox.accepts_prompts);
        assert_eq!(inbox.icon, None);
        let inbox = inbox.accepting_prompts().with_icon("/ext/demo/inbox.svg");
        assert!(inbox.accepts_prompts);
        assert_eq!(inbox.icon.as_deref(), Some("/ext/demo/inbox.svg"));
    }

    #[test]
    fn a_report_carries_its_sections_and_its_screenshot() {
        let report = BugReport::new(
            "inbox",
            "It breaks",
            "### Description\nIt breaks",
            BugReportFields::described("It breaks"),
        )
        .with_screenshot(Screenshot::new("image/png", b"\x89PNG".to_vec()));
        assert_eq!(report.fields.description.as_deref(), Some("It breaks"));
        assert_eq!(report.fields.prompts, None);
        assert_eq!(report.screenshot.unwrap().media_type, "image/png");
    }

    #[test]
    fn the_fields_read_back_with_their_absent_sections() {
        let fields: BugReportFields =
            serde_json::from_value(serde_json::json!({ "description": "x", "prompts": "p" }))
                .unwrap();
        assert_eq!(fields.prompts.as_deref(), Some("p"));
        assert_eq!(fields.run, None);
    }
}
