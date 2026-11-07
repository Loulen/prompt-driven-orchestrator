//! The CLI of the platform context: `pdo page` (a client of the daemon's
//! `/pages`), `pdo audit` (its journal), `pdo update` (its version check and
//! update), `pdo service` (runs on the host, no daemon involved), and
//! [`daemon`], the client every operator command of the other contexts
//! reaches the daemon with.

mod audit;
pub mod daemon;
mod page;
mod service;
mod text;
mod update;

use std::net::IpAddr;
use std::path::PathBuf;

use clap::Subcommand;

pub use audit::{run_audit, AuditArgs};
pub use page::run_page;
pub use service::run_service;
pub use update::{run_update, UpdateAction};

use crate::domain::service_unit::DEFAULT_PORT;

#[derive(Subcommand, Debug)]
pub enum PageAction {
    /// Bind a name to a directory and serve it under `/pages/<name>/`.
    Mount {
        name: String,
        directory: PathBuf,
        /// A reading guide (YAML or JSON): a list of entries `{title, text,
        /// link}`, shown beside the page in PDO. A link leads into PDO
        /// (`#/runs/<run>`, `#/runs/<run>/nodes/<node>`,
        /// `#/runs/<run>/nodes/<node>/iter/<n>`,
        /// `#/runs/<run>/nodes/<node>/artifacts/<port>`,
        /// `/runs/<run>/review?file=<path>`) or into the page (`#anchor`, a
        /// relative path).
        #[arg(long, value_name = "FILE")]
        guide: Option<PathBuf>,
    },
    /// List active page mounts.
    List,
    /// Remove a page mount.
    Unmount { name: String },
}

#[derive(Subcommand, Debug)]
pub enum ServiceAction {
    /// Generate + enable the service unit so the daemon starts at boot / survives logout.
    Install {
        /// Address the service daemon listens on. Omit to keep the default.
        #[arg(long)]
        bind: Option<IpAddr>,
        #[arg(short, long, env = "PDO_PORT", default_value_t = DEFAULT_PORT)]
        port: u16,
        /// Print the unit file + the exact commands, make NO changes. Still
        /// probes the port so the conflict guard is visible, but performs zero
        /// writes and runs no `systemctl`.
        #[arg(long)]
        dry_run: bool,
    },
    /// Stop, disable, and remove the service unit.
    Uninstall,
    /// Show the service unit's status (`systemctl --user status pdo` / launchd equivalent).
    Status,
}
