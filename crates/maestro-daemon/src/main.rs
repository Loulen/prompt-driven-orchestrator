use anyhow::{Context, Result};
use clap::Parser;
use maestro_daemon::{run_complete, run_daemon, run_fail, Cli, Commands};

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon { port } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "maestro_daemon=info,info".into()),
                )
                .with_writer(std::io::stderr)
                .init();
            // Only the daemon needs a tokio runtime. `run_complete` / `run_fail`
            // use `reqwest::blocking` and panic on shutdown if invoked from
            // within `#[tokio::main]`'s runtime context.
            tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .context("failed to build tokio runtime")?
                .block_on(run_daemon(port))
        }
        Commands::Complete => run_complete(),
        Commands::Fail { reason } => run_fail(reason),
    }
}
