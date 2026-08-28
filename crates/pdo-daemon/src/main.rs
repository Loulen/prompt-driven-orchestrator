use anyhow::{Context, Result};
use clap::Parser;
use pdo_daemon::{
    run_complete, run_daemon, run_docs, run_fail, run_migrate, run_reap, run_service, run_skip,
    Cli, Commands,
};
use std::process::ExitCode;

fn main() -> ExitCode {
    let cli = Cli::parse();

    // `complete` is the one subcommand with an exit-code contract (#490,
    // ADR-0035 §4): `0` granted / legal duplicate, `3` refused-still-your-turn,
    // `4` refused-already-ruled, `1` breakdown. Those codes live in pipeline
    // authors' bash — a `script` node's tail branches on the `4` to avoid doubling
    // a failure the daemon already recorded — so they are as much a public API as
    // the wire shape. It therefore owns its own return, and the other arms keep the
    // plain `Result` → `0`/`1` mapping they have always had.
    if let Commands::Complete { auto } = cli.command {
        return run_complete(auto);
    }

    let res: Result<()> = match cli.command {
        Commands::Daemon { port } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "pdo_daemon=info,info".into()),
                )
                .with_writer(std::io::stderr)
                .init();
            // Only the daemon needs a tokio runtime. `run_complete` / `run_fail`
            // use `reqwest::blocking` and panic on shutdown if invoked from
            // within `#[tokio::main]`'s runtime context.
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime")
                .and_then(|rt| rt.block_on(run_daemon(port)))
        }
        Commands::Complete { .. } => unreachable!("`complete` returns its own ExitCode"),
        Commands::Fail { reason } => run_fail(reason),
        Commands::Skip { reason } => run_skip(reason),
        // Every arm below is a blocking one-shot: no tokio runtime, for the
        // `reqwest::blocking` reason given above.
        Commands::Service { action } => run_service(action),
        Commands::Migrate { dir, dry_run } => run_migrate(dir, dry_run),
        Commands::Reap {
            count,
            dry_run,
            ttl_hours,
            terminal_ttl_hours,
            budget_secs,
        } => run_reap(count, dry_run, ttl_hours, terminal_ttl_hours, budget_secs),
        Commands::Docs { action } => run_docs(action),
    };

    if let Err(e) = res {
        eprintln!("Error: {e:?}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
