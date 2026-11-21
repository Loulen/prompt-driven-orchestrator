//! The Destinations de bug report of the extensions (API 0.28): what each
//! offers an Actor, and the report handed to the one it is sent to.

use std::panic::{catch_unwind, AssertUnwindSafe};

use pdo_extension_api::{BugReport, BugReportDestination, BugReportSent};

use super::host_of::panic_message;
use super::Extensions;
use crate::Actor;

/// A destination an extension offers, with the extension it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedDestination {
    /// The extension's id: the destination is `<extension>/<id>`.
    pub extension: String,
    /// The extension's name: the header « Send to » lists it under.
    pub extension_name: String,
    pub destination: BugReportDestination,
}

/// Why an extension did not receive a report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BugReportNotReceived {
    /// No extension of that id is loaded.
    UnknownExtension,
    /// The extension refused it, or panicked: its reason.
    Failed(String),
}

impl Extensions {
    /// Every extension's destinations for `actor`, in registration order; an
    /// extension that panics offers none.
    pub(super) fn offered_destinations(&self, actor: &Actor) -> Vec<OfferedDestination> {
        let mut offered = Vec::new();
        for r in &self.registry.registered {
            let id = &r.manifest.id;
            match catch_unwind(AssertUnwindSafe(|| {
                r.extension.bug_report_destinations(actor)
            })) {
                Ok(destinations) => {
                    offered.extend(
                        destinations
                            .into_iter()
                            .map(|destination| OfferedDestination {
                                extension: id.clone(),
                                extension_name: r.manifest.name.clone(),
                                destination,
                            }),
                    )
                }
                Err(panic) => tracing::error!(
                    "extension `{id}` panicked in bug_report_destinations ({}); it offers none",
                    panic_message(panic.as_ref())
                ),
            }
        }
        offered
    }

    /// Hand `report` to the extension `extension`. Blocking: call it off the
    /// request path.
    pub(super) fn receive_report(
        &self,
        actor: &Actor,
        extension: &str,
        report: &BugReport,
    ) -> Result<BugReportSent, BugReportNotReceived> {
        let r = self
            .registry
            .registered
            .iter()
            .find(|r| r.manifest.id == extension)
            .ok_or(BugReportNotReceived::UnknownExtension)?;
        match catch_unwind(AssertUnwindSafe(|| {
            r.extension.receive_bug_report(actor, report)
        })) {
            Ok(Ok(sent)) => Ok(sent),
            Ok(Err(reason)) => Err(BugReportNotReceived::Failed(reason)),
            Err(panic) => {
                tracing::error!(
                    "extension `{extension}` panicked in receive_bug_report ({})",
                    panic_message(panic.as_ref())
                );
                Err(BugReportNotReceived::Failed(format!(
                    "extension `{extension}` failed to receive the report"
                )))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extension_host::ExtensionHost;
    use pdo_extension_api::{BugReportFields, Extension, Manifest};

    struct Desk;
    impl Extension for Desk {
        fn manifest(&self) -> Manifest {
            Manifest::new("desk", "Desk", "0.0.0")
        }
        fn bug_report_destinations(&self, actor: &Actor) -> Vec<BugReportDestination> {
            match actor {
                Actor::LocalOperator => vec![
                    BugReportDestination::new("inbox", "Inbox").accepting_prompts(),
                    BugReportDestination::new("archive", "Archive"),
                ],
                _ => Vec::new(),
            }
        }
        fn receive_bug_report(
            &self,
            _actor: &Actor,
            report: &BugReport,
        ) -> Result<BugReportSent, String> {
            match report.destination.as_str() {
                "inbox" => Ok(BugReportSent::new("BUG-1", "https://desk.example/BUG-1")),
                _ => Err("the archive is read-only".into()),
            }
        }
    }

    struct Broken;
    impl Extension for Broken {
        fn manifest(&self) -> Manifest {
            Manifest::new("broken", "Broken", "0.0.0")
        }
        fn bug_report_destinations(&self, _actor: &Actor) -> Vec<BugReportDestination> {
            panic!("no list today")
        }
        fn receive_bug_report(
            &self,
            _actor: &Actor,
            _report: &BugReport,
        ) -> Result<BugReportSent, String> {
            panic!("no report today")
        }
    }

    fn report(destination: &str) -> BugReport {
        BugReport::new(destination, "t", "x", BugReportFields::described("x"))
    }

    fn host() -> Extensions {
        Extensions::register(vec![Box::new(Broken), Box::new(Desk)]).unwrap()
    }

    #[test]
    fn each_extension_offers_its_destinations_to_the_actor_a_panic_none() {
        let offered = host().bug_report_destinations(&Actor::LocalOperator);
        let ids: Vec<_> = offered
            .iter()
            .map(|o| format!("{}/{}", o.extension, o.destination.id))
            .collect();
        assert_eq!(ids, ["desk/inbox", "desk/archive"]);
        assert_eq!(offered[0].extension_name, "Desk");
        assert!(offered[0].destination.accepts_prompts);
        let other = Actor::Identified {
            namespace: "x".into(),
            id: "bob".into(),
        };
        assert!(host().bug_report_destinations(&other).is_empty());
    }

    #[test]
    fn the_report_reaches_its_extension_which_answers_or_refuses() {
        let host = host();
        let actor = Actor::LocalOperator;
        assert_eq!(
            host.receive_bug_report(&actor, "desk", &report("inbox")),
            Ok(BugReportSent::new("BUG-1", "https://desk.example/BUG-1"))
        );
        assert_eq!(
            host.receive_bug_report(&actor, "desk", &report("archive")),
            Err(BugReportNotReceived::Failed(
                "the archive is read-only".into()
            ))
        );
        assert_eq!(
            host.receive_bug_report(&actor, "nobody", &report("inbox")),
            Err(BugReportNotReceived::UnknownExtension)
        );
        assert_eq!(
            host.receive_bug_report(&actor, "broken", &report("inbox")),
            Err(BugReportNotReceived::Failed(
                "extension `broken` failed to receive the report".into()
            ))
        );
    }
}
