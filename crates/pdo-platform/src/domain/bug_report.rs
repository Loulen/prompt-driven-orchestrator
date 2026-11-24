//! The rules of a Rapport de bug sent to a Destination de bug report
//! (CONTEXT.md): how a destination is named, what a report must carry, and
//! why one is refused. The core's own destination, a new issue of its public
//! repository, is opened by the browser: the daemon only remembers it was the
//! last choice.

use pdo_extension_api::BugReportFields;

/// The id of the core's destination, the public issues.
pub const CORE_DESTINATION: &str = "github";

/// The largest screenshot a report carries.
pub const MAX_SCREENSHOT_BYTES: usize = 10 * 1024 * 1024;

/// The longest title a report gets from its description.
const TITLE_CHARS: usize = 80;

/// The id of an extension's destination: `<extension>/<id>`.
pub fn destination_id(extension: &str, id: &str) -> String {
    format!("{extension}/{id}")
}

/// The extension and the destination an id names; `None` for the core's or
/// a malformed one.
pub fn split_destination(id: &str) -> Option<(&str, &str)> {
    id.split_once('/')
        .filter(|(extension, destination)| !extension.is_empty() && !destination.is_empty())
}

/// The title of a report: its description's first line, shortened.
pub fn title_of(description: &str) -> String {
    let first = description
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("");
    if first.chars().count() > TITLE_CHARS {
        let kept: String = first.chars().take(TITLE_CHARS - 1).collect();
        format!("{kept}…")
    } else {
        first.to_string()
    }
}

/// Why a report was not sent, or a choice not remembered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BugReportRefusal {
    /// No destination of that id is offered to the caller.
    #[error("no bug report destination `{0}` for you")]
    UnknownDestination(String),
    /// The core's destination is opened by the browser, never sent to.
    #[error("the core's issues open in the browser: the daemon sends nothing there")]
    CoreDestination,
    #[error("{0} does not accept prompts: uncheck Include prompts")]
    PromptsRefused(String),
    #[error("describe the bug first")]
    NoDescription,
    #[error("the screenshot is not an image ({0})")]
    NotAnImage(String),
    #[error("the screenshot is not valid base64: {0}")]
    UnreadableScreenshot(String),
    #[error("the screenshot is over {} MB", MAX_SCREENSHOT_BYTES / (1024 * 1024))]
    ScreenshotTooLarge,
    /// The destination's extension refused it, or failed: its reason.
    #[error("{0}")]
    NotReceived(String),
    #[error("{0}")]
    Storage(String),
}

/// What a report must carry before it leaves, whatever its destination
/// accepts: a description, and the prompts only where they may go.
pub fn check_report(
    fields: &BugReportFields,
    destination_label: &str,
    accepts_prompts: bool,
) -> Result<(), BugReportRefusal> {
    if fields
        .description
        .as_deref()
        .is_none_or(|d| d.trim().is_empty())
    {
        return Err(BugReportRefusal::NoDescription);
    }
    if fields.prompts.is_some() && !accepts_prompts {
        return Err(BugReportRefusal::PromptsRefused(
            destination_label.to_string(),
        ));
    }
    Ok(())
}

/// What a screenshot must be: an image, within the size.
pub fn check_screenshot(media_type: &str, size: usize) -> Result<(), BugReportRefusal> {
    if !media_type.starts_with("image/") {
        return Err(BugReportRefusal::NotAnImage(media_type.to_string()));
    }
    if size > MAX_SCREENSHOT_BYTES {
        return Err(BugReportRefusal::ScreenshotTooLarge);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_destination_is_named_after_its_extension() {
        assert_eq!(destination_id("desk", "inbox"), "desk/inbox");
        assert_eq!(split_destination("desk/inbox"), Some(("desk", "inbox")));
        assert_eq!(split_destination("desk/a/b"), Some(("desk", "a/b")));
        assert_eq!(split_destination(CORE_DESTINATION), None);
        assert_eq!(split_destination("/inbox"), None);
        assert_eq!(split_destination("desk/"), None);
    }

    #[test]
    fn the_title_is_the_first_line_shortened() {
        assert_eq!(title_of("\n  It breaks  \nmore"), "It breaks");
        assert_eq!(title_of(""), "");
        let long = "x".repeat(100);
        let title = title_of(&long);
        assert_eq!(title.chars().count(), 80);
        assert!(title.ends_with('…'));
    }

    #[test]
    fn a_report_needs_a_description_and_its_prompts_a_destination_that_accepts_them() {
        let mut fields = BugReportFields::default();
        assert_eq!(
            check_report(&fields, "Inbox", true),
            Err(BugReportRefusal::NoDescription)
        );
        fields = BugReportFields::described("It breaks");
        assert_eq!(check_report(&fields, "Inbox", false), Ok(()));
        fields.prompts = Some("#### Node\nDo it".into());
        assert_eq!(check_report(&fields, "Inbox", true), Ok(()));
        assert_eq!(
            check_report(&fields, "Inbox", false),
            Err(BugReportRefusal::PromptsRefused("Inbox".into()))
        );
    }

    #[test]
    fn a_screenshot_is_an_image_within_the_size() {
        assert_eq!(check_screenshot("image/png", 10), Ok(()));
        assert_eq!(
            check_screenshot("text/plain", 10),
            Err(BugReportRefusal::NotAnImage("text/plain".into()))
        );
        assert_eq!(
            check_screenshot("image/png", MAX_SCREENSHOT_BYTES + 1),
            Err(BugReportRefusal::ScreenshotTooLarge)
        );
    }
}
