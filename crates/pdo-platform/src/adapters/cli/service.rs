//! `pdo service` — install, remove or inspect the daemon's OS service unit.

use anyhow::Result;
use pdo_kernel::extension_host::ExtensionHost;
use pdo_kernel::Actor;

use super::ServiceAction;
use crate::adapters::host_service::RealServiceHost;
use crate::application::install_service::{InstallService, InstallServiceCommand};
use crate::application::service_status::ServiceStatus;
use crate::application::uninstall_service::UninstallService;

/// Run `pdo service <action>` on this host, as the local operator, the trace on
/// stdout.
pub fn run_service(action: ServiceAction, extensions: &dyn ExtensionHost) -> Result<()> {
    let mut out = std::io::stdout();
    let host = RealServiceHost;
    let actor = Actor::LocalOperator;
    match action {
        ServiceAction::Install {
            bind,
            port,
            dry_run,
        } => InstallService {
            host: &host,
            extensions,
        }
        .execute(
            &actor,
            InstallServiceCommand {
                bind,
                port,
                dry_run,
            },
            &mut out,
        ),
        ServiceAction::Uninstall => UninstallService {
            host: &host,
            extensions,
        }
        .execute(&actor, &mut out),
        ServiceAction::Status => ServiceStatus { host: &host }.execute(&mut out),
    }
}
