//! The Destinations de bug report (CONTEXT.md): the ones the extensions offer
//! the caller and the last it chose (`GET /bug-report/destinations`), a report
//! sent to one of them (`POST /bug-report`), the core's remembered as the last
//! choice once the browser opened it (`PUT /bug-report/last`).
//!
//! No Operation is asked: each extension offers only the destinations the
//! Actor may send to, and the daemon sends only to one it offers that Actor.
//! The last choice is the Actor's own, like its Apparence: only a person
//! keeps one.

use std::sync::Arc;

use pdo_extension_api::{BugReport, BugReportFields, BugReportSent, Screenshot};
use pdo_kernel::extension_host::{BugReportNotReceived, ExtensionHost, OfferedDestination};
use pdo_kernel::Actor;

use super::ports::BugReportChoices;
use crate::domain::bug_report::{
    check_report, check_screenshot, destination_id, split_destination, title_of, BugReportRefusal,
    CORE_DESTINATION,
};

/// What the caller may send to, and where « Send to » starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destinations {
    pub offered: Vec<OfferedDestination>,
    /// The destination it chose last, when it is still there.
    pub last: Option<String>,
}

/// A report as the window sends it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingReport {
    /// `<extension>/<id>`.
    pub destination: String,
    pub text: String,
    pub fields: BugReportFields,
    pub screenshot: Option<Screenshot>,
}

pub struct BugReports {
    choices: Arc<dyn BugReportChoices>,
    extensions: Arc<dyn ExtensionHost>,
}

/// Whether `actor` keeps a last choice: a person, never an agent or a Trigger.
fn keeps_choices(actor: &Actor) -> bool {
    matches!(actor, Actor::LocalOperator | Actor::Identified { .. })
}

impl BugReports {
    pub fn new(choices: Arc<dyn BugReportChoices>, extensions: Arc<dyn ExtensionHost>) -> Self {
        Self {
            choices,
            extensions,
        }
    }

    pub async fn init(&self) -> Result<(), BugReportRefusal> {
        self.choices
            .init()
            .await
            .map_err(|e| BugReportRefusal::Storage(e.0))
    }

    /// The destinations the extensions offer `actor`, and its last choice
    /// when it is the core's or one still offered.
    pub async fn destinations(&self, actor: &Actor) -> Destinations {
        let offered = self.extensions.bug_report_destinations(actor);
        let last = match self.choices.last(actor).await {
            Ok(last) => last,
            Err(e) => {
                tracing::warn!("the last bug report destination could not be read: {e}");
                None
            }
        };
        let last = last.filter(|id| id == CORE_DESTINATION || find(&offered, id).is_some());
        Destinations { offered, last }
    }

    /// Send `report` to the destination it names, once checked; remembers it
    /// as `actor`'s last choice when the extension received it. The extension
    /// runs on a blocking thread, which a caller that hangs up does not stop:
    /// the item is created all the same, without its answer reaching anyone.
    pub async fn send(
        &self,
        actor: &Actor,
        report: OutgoingReport,
    ) -> Result<BugReportSent, BugReportRefusal> {
        if report.destination == CORE_DESTINATION {
            return Err(BugReportRefusal::CoreDestination);
        }
        let offered = self.extensions.bug_report_destinations(actor);
        let target = find(&offered, &report.destination)
            .ok_or_else(|| BugReportRefusal::UnknownDestination(report.destination.clone()))?;
        check_report(
            &report.fields,
            &target.destination.label,
            target.destination.accepts_prompts,
        )?;
        if let Some(screenshot) = &report.screenshot {
            check_screenshot(&screenshot.media_type, screenshot.bytes.len())?;
        }
        let title = title_of(report.fields.description.as_deref().unwrap_or(""));
        let mut outgoing = BugReport::new(
            target.destination.id.clone(),
            title,
            report.text,
            report.fields,
        );
        if let Some(screenshot) = report.screenshot {
            outgoing = outgoing.with_screenshot(screenshot);
        }
        let extensions = self.extensions.clone();
        let extension = target.extension.clone();
        let sender = actor.clone();
        let received = tokio::task::spawn_blocking(move || {
            extensions.receive_bug_report(&sender, &extension, &outgoing)
        })
        .await
        .map_err(|e| BugReportRefusal::NotReceived(format!("the report was lost: {e}")))?;
        let sent = received.map_err(|e| match e {
            BugReportNotReceived::UnknownExtension => {
                BugReportRefusal::UnknownDestination(report.destination.clone())
            }
            BugReportNotReceived::Failed(reason) => BugReportRefusal::NotReceived(reason),
        })?;
        self.keep(actor, &report.destination).await;
        Ok(sent)
    }

    /// Remember `destination` as `actor`'s last choice: the core's, or one
    /// the extensions offer it.
    pub async fn remember(&self, actor: &Actor, destination: &str) -> Result<(), BugReportRefusal> {
        if destination != CORE_DESTINATION
            && find(&self.extensions.bug_report_destinations(actor), destination).is_none()
        {
            return Err(BugReportRefusal::UnknownDestination(
                destination.to_string(),
            ));
        }
        if keeps_choices(actor) {
            self.choices
                .remember(actor, destination)
                .await
                .map_err(|e| BugReportRefusal::Storage(e.0))?;
        }
        Ok(())
    }

    /// Best-effort: a report already sent is not failed for its choice.
    async fn keep(&self, actor: &Actor, destination: &str) {
        if !keeps_choices(actor) {
            return;
        }
        if let Err(e) = self.choices.remember(actor, destination).await {
            tracing::warn!("the last bug report destination could not be kept: {e}");
        }
    }
}

/// The destination `id` (`<extension>/<id>`) among `offered`.
fn find<'a>(offered: &'a [OfferedDestination], id: &str) -> Option<&'a OfferedDestination> {
    let (extension, destination) = split_destination(id)?;
    offered
        .iter()
        .find(|o| o.extension == extension && o.destination.id == destination)
}

/// The id of an offered destination, as the window and the CLI name it.
pub fn offered_id(offered: &OfferedDestination) -> String {
    destination_id(&offered.extension, &offered.destination.id)
}
