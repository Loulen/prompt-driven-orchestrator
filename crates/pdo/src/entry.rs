//! `main_with(extensions)`: the whole `pdo` binary (ADR-0081). PDO's own
//! `main` calls it with no extension; a third-party distribution calls it with
//! its own, and gets the same CLI, the same exit codes and the same daemon.

use std::ffi::OsString;
use std::net::IpAddr;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::Parser;
use pdo_extension_api::Extension;
use pdo_kernel::extension_host::Extensions;
use pdo_pipeline::adapters::cli::run_pipeline;
use pdo_platform::adapters::cli::daemon::exit_code;
use pdo_platform::adapters::cli::{run_audit, run_page, run_service, run_update, ServiceAction};
use pdo_review::adapters::cli::run_review;
use pdo_run::adapters::cli::{
    refused_in_hosted_session, run_complete, run_fail, run_node, run_reap, run_run_create,
    run_run_gesture, run_run_list, run_run_read, run_run_status, run_run_wait, run_skip,
    run_wait_user, RunAction,
};

use pdo_session::adapters::cli::run_session;
use pdo_settings::adapters::cli::run_settings;
use pdo_skill::adapters::cli::run_skill;
use pdo_stats::adapters::cli::run_stats;
use pdo_trigger::adapters::cli::run_trigger;
use pdo_workspace::adapters::cli::run_project;

use crate::cli::{run_docs, run_ext, run_migrate, run_whoami, Cli, Commands};

/// The whole `pdo` binary: parse the command line, run the subcommand, return
/// its exit code.
///
/// `extensions` builds the extensions of this binary. It is called at most
/// once, and only by the subcommands that need them: `pdo daemon` serves with
/// them (it calls `extensions` inside the daemon's tokio runtime), and
/// `pdo service` submits its install and uninstall to their Autorisation
/// d'opération (outside any runtime). The agent's CLI (`pdo complete`, `fail`,
/// `wait-user`, `run create`, `ext`…) never builds them: `pdo ext` relays its
/// call to the daemon, whose extensions answer it.
///
/// The exit codes are a public contract, the same for every binary:
///
/// - `pdo complete`: `0` granted or legal duplicate, `3` refused and still your
///   turn, `4` refused and the runtime already ruled, `1` no verdict (ADR-0035
///   §4);
/// - `pdo wait-user`: `3` refused, still your turn; `pdo run wait`: `2` timed
///   out, not a failure (ADR-0069);
/// - `pdo complete`, `fail` and `skip` from a Session hébergée: `3`, refused,
///   nothing changed (ADR-0092);
/// - `pdo ext <id> <args…>`: the extension's own code, relayed as is; `1` the
///   daemon cannot be reached or the extension failed; `127` no extension
///   `<id>`, or one without a command line (#33);
/// - `pdo run status`, the gestures of `pdo run`, `pdo node`, `pdo pipeline`,
///   `pdo project`, `pdo trigger`, `pdo session`, the gestures of `pdo review`,
///   `pdo settings`, `pdo stats`, `pdo skill`, `pdo update`: `4` the daemon
///   knows no such subject (Run, node, pipeline, Projet, Trigger, session,
///   comment, profile, skill, folder, update attempt);
/// - every other subcommand: `0` success, `1` error; `--help` and `--version`
///   exit `0`, a command line clap cannot read exits `2`.
///
/// ```no_run
/// fn main() -> std::process::ExitCode {
///     pdo::main_with(Vec::new)
/// }
/// ```
pub fn main_with(extensions: impl FnOnce() -> Vec<Box<dyn Extension>>) -> ExitCode {
    main_with_config(extensions, |config| config)
}

/// [`main_with`], with `configure` applied to the [`Config`](crate::Config)
/// of `pdo daemon` before it serves: the way a distribution declares what
/// `serve` would take, such as its [`ReleaseChannel`](crate::ReleaseChannel)
/// or its [`Brand`](crate::Brand).
/// Called at most once, by `pdo daemon` only.
///
/// ```no_run
/// fn main() -> std::process::ExitCode {
///     pdo::main_with_config(Vec::new, |config| {
///         config
///             .with_release_channel(pdo::ReleaseChannel {
///                 name: "acme-pdo".into(),
///                 installed_version: env!("CARGO_PKG_VERSION").into(),
///                 source_url: None,
///                 upgrade_command: None,
///             })
///             .with_brand(pdo::Brand::AcogniDriver)
///     })
/// }
/// ```
pub fn main_with_config(
    extensions: impl FnOnce() -> Vec<Box<dyn Extension>>,
    configure: impl FnOnce(crate::Config) -> crate::Config,
) -> ExitCode {
    main_from_config(std::env::args_os(), extensions, configure)
}

/// Not covered by semver: [`main_with`] on a given command line (`args`
/// starts with the program name). The test suite runs it in process with the
/// internal test extension.
#[doc(hidden)]
pub fn main_from<I, T>(args: I, extensions: impl FnOnce() -> Vec<Box<dyn Extension>>) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    main_from_config(args, extensions, |config| config)
}

/// Not covered by semver: [`main_with_config`] on a given command line.
#[doc(hidden)]
pub fn main_from_config<I, T>(
    args: I,
    extensions: impl FnOnce() -> Vec<Box<dyn Extension>>,
    configure: impl FnOnce(crate::Config) -> crate::Config,
) -> ExitCode
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        // `--help`, `--version` and a bad command line: printed, and coded,
        // exactly as `Cli::parse` would, without leaving the process.
        Err(e) => {
            let _ = e.print();
            return ExitCode::from(u8::try_from(e.exit_code()).unwrap_or(1));
        }
    };

    // Every arm runs a blocking one-shot, with no tokio runtime, except
    // `daemon`: `reqwest::blocking` (the agent's CLI, the service's port probe)
    // panics when called from within a runtime.
    match cli.command {
        Commands::Daemon { bind, port } => exit(run_daemon(bind, port, extensions, configure)),
        // `complete` owns its exit code (see above): those codes live in
        // pipeline authors' bash — a `script` node's tail branches on the `4` to
        // avoid doubling a failure the daemon already recorded.
        Commands::Complete { auto } => run_complete(auto),
        // `wait-user` shares the `3` of that contract.
        Commands::WaitUser { message } => run_wait_user(message),
        // A Session hébergée is never completed, failed nor skipped
        // (ADR-0092): refused with the `3` of that contract.
        Commands::Fail { reason } => {
            refused_in_hosted_session("fail").unwrap_or_else(|| exit(run_fail(reason)))
        }
        Commands::Skip { reason } => {
            refused_in_hosted_session("skip").unwrap_or_else(|| exit(run_skip(reason)))
        }
        Commands::Service { action } => exit(run_service_with(action, extensions)),
        Commands::Migrate { dir, dry_run } => exit(run_migrate(dir, dry_run)),
        Commands::Reap {
            count,
            dry_run,
            ttl_hours,
            terminal_ttl_hours,
            budget_secs,
        } => exit(run_reap(
            count,
            dry_run,
            ttl_hours,
            terminal_ttl_hours,
            budget_secs,
        )),
        Commands::Docs { action } => exit(run_docs(action)),
        Commands::Run { action } => match *action {
            // `run wait` owns the `2` (timeout, not a failure).
            RunAction::Wait { all, timeout } => run_run_wait(all, timeout),
            RunAction::List { json } => exit(run_run_list(json)),
            // `run status` owns the `4` (no such Run).
            RunAction::Status { run_id, json } => run_run_status(&run_id, json),
            // The operator's gestures share the `4` of `run status`.
            RunAction::Gesture(gesture) => exit_code(run_run_gesture(gesture)),
            RunAction::Read(read) => exit_code(run_run_read(read)),
            create @ RunAction::Create { .. } => exit(run_run_create(create)),
        },
        Commands::Page { action } => exit(run_page(action)),
        Commands::Review { action } => exit_code(run_review(action)),
        // `ext` owns its exit code: the extension's, relayed as is.
        Commands::Ext { args } => run_ext(args),
        // The operator's commands share the `4` of `run status`: no such
        // Projet, Trigger or session.
        Commands::Project { action } => exit_code(run_project(action)),
        Commands::Trigger { action } => exit_code(run_trigger(*action)),
        Commands::Node { action } => exit_code(run_node(action)),
        Commands::Pipeline { action } => exit_code(run_pipeline(action)),
        Commands::Session { action } => exit_code(run_session(action)),
        Commands::Settings { action } => exit_code(run_settings(*action)),
        Commands::Stats { action } => exit_code(run_stats(*action)),
        Commands::Skill { action } => exit_code(run_skill(*action)),
        Commands::Update { action } => exit_code(run_update(action)),
        Commands::Audit(args) => exit_code(run_audit(args)),
        Commands::Whoami { json } => exit_code(run_whoami(json)),
    }
}

/// The plain mapping of a subcommand without an exit-code contract: `0`, or
/// the error on stderr and `1`.
fn exit(res: Result<()>) -> ExitCode {
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Error: {e:?}");
            ExitCode::FAILURE
        }
    }
}

/// `pdo daemon`: [`crate::serve`] with the binary's extensions and its
/// configured [`Config`](crate::Config), in the one
/// tokio runtime of the process, until the daemon stops.
fn run_daemon(
    bind: Option<IpAddr>,
    port: u16,
    extensions: impl FnOnce() -> Vec<Box<dyn Extension>>,
    configure: impl FnOnce(crate::Config) -> crate::Config,
) -> Result<()> {
    // `try_init`: a subscriber the binary set up before `main_with` wins.
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("failed to build tokio runtime")?
        .block_on(async {
            let config = configure(crate::Config::from_cli(bind, port)?);
            crate::serve(config, extensions()).await?.wait().await
        })
}

/// `pdo service`: the binary's extensions rule on the install and the
/// uninstall, as they rule on every Operation of its daemon.
fn run_service_with(
    action: ServiceAction,
    extensions: impl FnOnce() -> Vec<Box<dyn Extension>>,
) -> Result<()> {
    let extensions =
        Extensions::register(extensions()).context("`pdo service` refused an extension")?;
    run_service(action, &extensions)
}
