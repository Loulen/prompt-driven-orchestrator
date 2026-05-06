use anyhow::Result;
use clap::Parser;
use maestro_daemon::{run_complete, run_daemon, run_fail, Cli, Commands};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Daemon { port } => {
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "maestro_daemon=info".into()),
                )
                .init();
            run_daemon(port).await
        }
        Commands::Complete => run_complete(),
        Commands::Fail { reason } => run_fail(reason),
    }
}
