//! The `pdo` command line: the daemon, the agent CLI of a node session, and the
//! one-shot commands of an operator. Each subcommand lives with its context
//! (`pdo_run::adapters::cli`, `pdo_platform::adapters::cli`, …); this module only
//! declares them, plus the two that belong to no context (`docs`, `migrate`).

mod docs;
mod ext;
mod migrate;

use std::net::IpAddr;

use clap::{Parser, Subcommand};
use pdo_platform::adapters::cli::{PageAction, ServiceAction};
use pdo_review::adapters::cli::ReviewAction;
use pdo_run::adapters::cli::RunAction;

pub use docs::run_docs;
pub use ext::run_ext;
pub use migrate::run_migrate;

const DEFAULT_PORT: u16 = pdo_platform::domain::service_unit::DEFAULT_PORT;

#[derive(Parser, Debug)]
#[command(
    name = "pdo",
    about = "PDO — deterministic Claude Code pipeline orchestrator",
    version
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Start the PDO daemon
    Daemon {
        /// Address to listen on. Defaults to all interfaces.
        #[arg(long, env = "PDO_BIND")]
        bind: Option<IpAddr>,
        #[arg(short, long, env = "PDO_PORT", default_value_t = DEFAULT_PORT)]
        port: u16,
    },
    /// Signal that the current NodeRun has completed successfully
    Complete {
        /// Runtime-initiated completion on turn end: set by the injected `Stop`
        /// hook, never by an agent typing `pdo complete`. Records
        /// `NodeAutoCompleted` (source `StopHook`).
        #[arg(long)]
        auto: bool,
    },
    /// Signal that the current NodeRun has failed
    Fail {
        #[arg(long)]
        reason: String,
    },
    /// Declare that you are waiting on the user (#588, ADR-0069): the node and
    /// its run turn `awaiting_user`, the message is shown on the run's banner.
    /// Accepted on any node with a live session; refused (exit 3) on a `script`
    /// node or without a live session; a repeat is a no-op. The wait lifts when
    /// the user answers in the PDO terminal (Enter) or releases completion.
    WaitUser {
        /// The question you are waiting on, shown verbatim on the banner
        /// (capped at 100 characters).
        #[arg(long)]
        message: Option<String>,
    },
    /// Graceful no-op: there is legitimately nothing to do, so end the run as
    /// `skipped` instead of `failed`. Use when the input/pool is empty, NOT for
    /// genuine errors — reserve `fail` for those.
    Skip {
        #[arg(long)]
        reason: String,
    },
    /// Migrate legacy pipeline YAMLs and node-library entries on disk
    /// (ADR-0011): dissolve `type: for-each` (and other retired control nodes)
    /// into `loops:` regions, rewiring edges; move a port's `side` onto its edges
    /// and drop its `repeated`. Never auto-run at load — a migration is always a
    /// deliberate act. Reports what changed in each file. A real run writes a
    /// `.bak` beside each migrated file.
    Migrate {
        /// Migrate only this directory of pipeline YAMLs. Default: the repo's
        /// `.pdo/pipelines`, `~/.pdo/pipelines`, and the library store.
        #[arg(long)]
        dir: Option<std::path::PathBuf>,
        /// Report what would be migrated without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Reclaim disk from old terminal Runs surfaced by `GET /runs/reapable`.
    /// Applies a graded-TTL policy ([`reap_policy`]) and archives each match via
    /// the existing `cleanup_run` command.
    ///
    /// Driven from a pipeline `script` node, never by the runtime itself
    /// (ADR-0012a: the runtime never triggers a durable action on its own).
    Reap {
        /// Print only the number of Runs the policy would reclaim, then exit 0.
        /// A Trigger guard can gate on it: `[ "$(pdo reap --count)" -gt 0 ]`.
        #[arg(long)]
        count: bool,
        /// Show the reclaim plan without archiving anything.
        #[arg(long)]
        dry_run: bool,
        /// TTL in hours for `completed` Runs (default 24).
        #[arg(long)]
        ttl_hours: Option<i64>,
        /// TTL in hours for `failed`/`halted`/`skipped` Runs — post-mortem
        /// evidence, held longer but bounded (default 72).
        #[arg(long)]
        terminal_ttl_hours: Option<i64>,
        /// Wall-clock budget in seconds for issuing reclaims (default 40). Must
        /// stay under `SCRIPT_TIMEOUT_SECS` (60s), which kills the janitor's
        /// `script` node; past the budget the janitor stops and still exits 0,
        /// because a failed janitor Run leaks its own worktree.
        #[arg(long, default_value_t = 40)]
        budget_secs: u64,
    },
    /// Manage the persistent OS service unit for the daemon (ADR-0019).
    ///
    /// Installs the daemon as a `systemd --user` unit (Linux) or a launchd
    /// LaunchAgent (macOS, best-effort) so it starts at boot and survives logout.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
    /// Print the documentation PDO generates from its own code.
    Docs {
        #[command(subcommand)]
        action: DocsAction,
    },
    /// Talk to a running daemon's Run surface.
    Run {
        #[command(subcommand)]
        action: Box<RunAction>,
    },
    /// Mount directories as live pages through the daemon.
    Page {
        #[command(subcommand)]
        action: PageAction,
    },
    /// Run an extension's command line through the daemon: `pdo ext <id>
    /// <args…>`. Arguments and standard input go to the extension with the
    /// node session the command runs in; its stdout, stderr and exit code
    /// come back as they are. `pdo ext` alone lists the extensions that have
    /// a command line. Exits 1 when the daemon cannot be reached, 127 when it
    /// has no extension `<id>` or one without a command line.
    Ext {
        /// The extension's id, then its arguments, verbatim.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Read and answer the review comments a human left on this Run's diff
    /// (#751, ADR-0067 §4). Run from a node or manager session: the Run id comes
    /// from `PDO_RUN_ID` (`--run` overrides it) and the author from the session
    /// (`manager`, or the node id).
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
}

#[derive(Subcommand, Debug)]
pub enum DocsAction {
    /// The harness support matrix — which capability each harness has, what is
    /// absent and why, and the last validated version of each binary.
    ///
    /// Rendered from the capability declaration in `pdo_harness::probes`, its only
    /// source of truth. `--check` fails (exit 1) if the block committed in the
    /// target file has drifted from the code; `--write` puts it back.
    SupportTable {
        /// Fail if the block in the target file has drifted from the code.
        #[arg(long)]
        check: bool,
        /// Rewrite the block in the target file from the code.
        #[arg(long, conflicts_with = "check")]
        write: bool,
        /// The file carrying the generated block, between its two markers.
        #[arg(long, default_value = pdo_harness::support::DOCUMENT)]
        file: std::path::PathBuf,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_parses_daemon_subcommand() {
        let cli = Cli::try_parse_from(["pdo", "daemon"]).unwrap();
        // With no `--port`, clap resolves the port from `PDO_PORT` if set,
        // otherwise `DEFAULT_PORT` (see the `#[arg(env = "PDO_PORT", ...)]`
        // on the Daemon variant). Compute the expectation the same way so the
        // test is deterministic whether or not the ambient env carries
        // `PDO_PORT` — e.g. when the suite itself runs inside a PDO node.
        let expected = std::env::var("PDO_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        match cli.command {
            Commands::Daemon { bind, port } => {
                assert_eq!(
                    bind,
                    std::env::var("PDO_BIND").ok().and_then(|v| v.parse().ok())
                );
                assert_eq!(port, expected);
            }
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_daemon_with_port() {
        let cli = Cli::try_parse_from(["pdo", "daemon", "--port", "9999"]).unwrap();
        match cli.command {
            Commands::Daemon { port, .. } => assert_eq!(port, 9999),
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_daemon_with_bind() {
        let cli = Cli::try_parse_from(["pdo", "daemon", "--bind", "127.0.0.1", "--port", "9999"])
            .unwrap();
        match cli.command {
            Commands::Daemon { bind, port } => {
                assert_eq!(bind, Some("127.0.0.1".parse().unwrap()));
                assert_eq!(port, 9999);
            }
            _ => panic!("expected Daemon subcommand"),
        }
    }

    #[test]
    fn cli_parses_complete_subcommand() {
        // Bare `complete` ⇒ `auto = false` (the agent-typed path, unchanged).
        let cli = Cli::try_parse_from(["pdo", "complete"]).unwrap();
        assert!(matches!(cli.command, Commands::Complete { auto: false }));
    }

    #[test]
    fn cli_parses_complete_auto_flag() {
        // `--auto` is the runtime-initiated (`Stop` hook) path.
        let cli = Cli::try_parse_from(["pdo", "complete", "--auto"]).unwrap();
        assert!(matches!(cli.command, Commands::Complete { auto: true }));
    }

    #[test]
    fn cli_parses_fail_subcommand() {
        let cli = Cli::try_parse_from(["pdo", "fail", "--reason", "timeout"]).unwrap();
        match cli.command {
            Commands::Fail { reason } => assert_eq!(reason, "timeout"),
            _ => panic!("expected Fail subcommand"),
        }
    }

    #[test]
    fn cli_fail_requires_reason() {
        assert!(Cli::try_parse_from(["pdo", "fail"]).is_err());
    }

    #[test]
    fn cli_parses_reap_with_defaults() {
        let cli = Cli::try_parse_from(["pdo", "reap"]).unwrap();
        match cli.command {
            Commands::Reap {
                count,
                dry_run,
                ttl_hours,
                terminal_ttl_hours,
                budget_secs,
            } => {
                assert!(!count);
                assert!(!dry_run);
                assert_eq!(ttl_hours, None);
                assert_eq!(terminal_ttl_hours, None);
                assert_eq!(budget_secs, 40, "the wall-clock budget defaults to 40s");
            }
            _ => panic!("expected Reap subcommand"),
        }
    }

    #[test]
    fn cli_parses_reap_flags() {
        let cli = Cli::try_parse_from([
            "pdo",
            "reap",
            "--count",
            "--dry-run",
            "--ttl-hours",
            "1",
            "--terminal-ttl-hours",
            "48",
            "--budget-secs",
            "10",
        ])
        .unwrap();
        match cli.command {
            Commands::Reap {
                count,
                dry_run,
                ttl_hours,
                terminal_ttl_hours,
                budget_secs,
            } => {
                assert!(count);
                assert!(dry_run);
                assert_eq!(ttl_hours, Some(1));
                assert_eq!(terminal_ttl_hours, Some(48));
                assert_eq!(budget_secs, 10);
            }
            _ => panic!("expected Reap subcommand"),
        }
    }

    #[test]
    fn cli_parses_service_install() {
        let cli = Cli::try_parse_from(["pdo", "service", "install"]).unwrap();
        let expected_port = std::env::var("PDO_PORT")
            .ok()
            .and_then(|v| v.parse::<u16>().ok())
            .unwrap_or(DEFAULT_PORT);
        match cli.command {
            Commands::Service {
                action:
                    ServiceAction::Install {
                        bind,
                        port,
                        dry_run,
                    },
            } => {
                assert_eq!(bind, None);
                assert_eq!(port, expected_port);
                assert!(!dry_run);
            }
            _ => panic!("expected Service Install"),
        }
    }

    #[test]
    fn cli_parses_service_install_with_port_and_dry_run() {
        let cli = Cli::try_parse_from(["pdo", "service", "install", "--port", "6183", "--dry-run"])
            .unwrap();
        match cli.command {
            Commands::Service {
                action:
                    ServiceAction::Install {
                        bind,
                        port,
                        dry_run,
                    },
            } => {
                assert_eq!(
                    bind,
                    std::env::var("PDO_BIND").ok().and_then(|v| v.parse().ok())
                );
                assert_eq!(port, 6183);
                assert!(dry_run);
            }
            _ => panic!("expected Service Install"),
        }
    }

    #[test]
    fn cli_parses_service_install_with_bind() {
        let cli = Cli::try_parse_from([
            "pdo",
            "service",
            "install",
            "--bind",
            "127.0.0.1",
            "--port",
            "6183",
            "--dry-run",
        ])
        .unwrap();
        match cli.command {
            Commands::Service {
                action:
                    ServiceAction::Install {
                        bind,
                        port,
                        dry_run,
                    },
            } => {
                assert_eq!(bind, Some("127.0.0.1".parse().unwrap()));
                assert_eq!(port, 6183);
                assert!(dry_run);
            }
            _ => panic!("expected Service Install"),
        }
    }

    #[test]
    fn cli_parses_service_uninstall() {
        let cli = Cli::try_parse_from(["pdo", "service", "uninstall"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Service {
                action: ServiceAction::Uninstall
            }
        ));
    }

    #[test]
    fn cli_parses_service_status() {
        let cli = Cli::try_parse_from(["pdo", "service", "status"]).unwrap();
        assert!(matches!(
            cli.command,
            Commands::Service {
                action: ServiceAction::Status
            }
        ));
    }

    #[test]
    fn cli_service_requires_an_action() {
        // `pdo service` with no subcommand is a usage error, not a silent no-op.
        assert!(Cli::try_parse_from(["pdo", "service"]).is_err());
    }

    #[test]
    fn cli_parses_run_create_bare() {
        // The one-argument common case: pipeline positional, everything else
        // deferred to the daemon (empty input, session-derived parent).
        let cli = Cli::try_parse_from(["pdo", "run", "create", "my-pipeline"]).unwrap();
        let Commands::Run { action } = cli.command else {
            panic!("expected Run subcommand")
        };
        let RunAction::Create {
            pipeline,
            input,
            variables,
            skills,
            harness,
            target_repo,
            name,
            ..
        } = *action
        else {
            panic!("expected `run create`");
        };
        assert_eq!(pipeline, "my-pipeline");
        assert_eq!(
            input, "",
            "empty input defers to the pipeline's prompt_required"
        );
        assert!(variables.is_none());
        assert!(skills.is_none());
        assert!(harness.is_none());
        assert!(target_repo.is_none());
        assert!(name.is_none());
    }

    #[test]
    fn cli_parses_run_create_flags() {
        let cli = Cli::try_parse_from([
            "pdo",
            "run",
            "create",
            "my-pipeline",
            "--input",
            "do the work",
            "--variables",
            r#"{"pool":"a"}"#,
            "--skills",
            "alpha,beta",
            "--agent-choice",
            r#"{"mode":"custom","harness":"pi"}"#,
            "--harness",
            "pi",
            "--sandbox",
            "off",
            "--target-repo",
            "/somewhere/repo",
            "--project",
            "prj-1",
            "--target-repos",
            r#"[{"repo":"/somewhere/other","read_only":true}]"#,
            "--source-branch",
            "feat/x",
            "--name",
            "child run",
            "--auto-name",
            "false",
            "--auto-fail",
            "true",
            "--provisioning",
            r#"{}"#,
            "--ext",
            "test.cle=valeur",
            "--ext",
            "test2.x=y",
        ])
        .unwrap();
        let Commands::Run { action } = cli.command else {
            panic!("expected Run subcommand")
        };
        let RunAction::Create {
            pipeline,
            input,
            input_file,
            image,
            file,
            variables,
            skills,
            agent_choice,
            harness,
            sandbox,
            target_repo,
            project,
            target_repos,
            source_branch,
            name,
            auto_name,
            auto_fail,
            provisioning,
            ext,
        } = *action
        else {
            panic!("expected `run create`");
        };
        assert_eq!(ext, vec!["test.cle=valeur", "test2.x=y"], "--ext repeats");
        assert_eq!(pipeline, "my-pipeline");
        assert_eq!(input, "do the work");
        assert!(input_file.is_none());
        assert!(image.is_empty());
        assert!(file.is_empty());
        assert_eq!(variables.as_deref(), Some(r#"{"pool":"a"}"#));
        assert_eq!(skills.as_deref(), Some("alpha,beta"));
        assert_eq!(
            agent_choice.as_deref(),
            Some(r#"{"mode":"custom","harness":"pi"}"#)
        );
        assert_eq!(harness.as_deref(), Some("pi"));
        assert_eq!(sandbox.as_deref(), Some("off"));
        assert_eq!(target_repo.as_deref(), Some("/somewhere/repo"));
        assert_eq!(project.as_deref(), Some("prj-1"));
        assert_eq!(
            target_repos.as_deref(),
            Some(r#"[{"repo":"/somewhere/other","read_only":true}]"#)
        );
        assert_eq!(source_branch.as_deref(), Some("feat/x"));
        assert_eq!(name.as_deref(), Some("child run"));
        assert_eq!(auto_name, Some(false));
        assert_eq!(auto_fail, Some(true));
        assert_eq!(provisioning.as_deref(), Some("{}"));
    }

    #[test]
    fn cli_parses_run_create_attachments() {
        // #779: `--image` / `--file` repeat, `--input-file` replaces `--input`.
        let cli = Cli::try_parse_from([
            "pdo",
            "run",
            "create",
            "my-pipeline",
            "--input-file",
            "/tmp/spec.md",
            "--image",
            "/tmp/a.png",
            "--image",
            "/tmp/b.png",
            "--file",
            "/tmp/fixtures.json",
        ])
        .unwrap();
        let Commands::Run { action } = cli.command else {
            panic!("expected Run subcommand")
        };
        let RunAction::Create {
            input_file,
            image,
            file,
            ..
        } = *action
        else {
            panic!("expected `run create`");
        };
        assert_eq!(
            input_file.as_deref(),
            Some(std::path::Path::new("/tmp/spec.md"))
        );
        assert_eq!(image.len(), 2);
        assert_eq!(file.len(), 1);
    }

    #[test]
    fn cli_refuses_input_and_input_file_together() {
        assert!(Cli::try_parse_from([
            "pdo",
            "run",
            "create",
            "p",
            "--input",
            "x",
            "--input-file",
            "/tmp/spec.md",
        ])
        .is_err());
    }

    #[test]
    fn cli_run_requires_an_action() {
        assert!(Cli::try_parse_from(["pdo", "run"]).is_err());
    }

    #[test]
    fn cli_version_flag() {
        let result = Cli::try_parse_from(["pdo", "--version"]);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }
}
