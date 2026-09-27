//! The demo daemon of the Feature Paths (#930): `pdo daemon`, plus the test
//! extension. Ignored by the suite, it serves until killed:
//!
//! ```sh
//! PDO_DEMO_PORT=6280 PDO_DEMO_ROOT=/path/to/state PDO_TEST_EXTENSION_REFUSE=project:update \
//!   cargo test -p pdo-daemon --test it demo_daemon -- --ignored --nocapture
//! ```
//!
//! - `PDO_DEMO_PORT`: the port, `6280` by default. It binds `127.0.0.1`.
//! - `PDO_DEMO_ROOT`: the directory the daemon keeps its state under, like the
//!   working directory of `pdo daemon`; the current one by default. Pipelines
//!   are read from `$PDO_DEMO_ROOT/.pdo/pipelines`, not from `$HOME`.
//! - `PDO_TEST_EXTENSION_REFUSE`: what the test extension refuses (`none`, `all`,
//!   `panic`, `<resource>:<verb>`), `project:update` by default.
//!
//! With `--exact`, the filter is the full path `demo_daemon::demo_daemon`.
//!
//! The caller named by the `x-pdo-test-actor` header is the identified Actor.

use std::net::{IpAddr, Ipv4Addr};

use crate::test_extension::{Refusal, TestExtension};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "serves until killed: the demo daemon of the Feature Paths"]
async fn demo_daemon() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pdo_daemon=info,info".into()),
        )
        .with_writer(std::io::stderr)
        .try_init();

    let port: u16 = std::env::var("PDO_DEMO_PORT")
        .ok()
        .map(|p| p.parse().expect("PDO_DEMO_PORT is a port"))
        .unwrap_or(6280);
    if let Ok(root) = std::env::var("PDO_DEMO_ROOT") {
        std::fs::create_dir_all(&root).unwrap();
        std::env::set_current_dir(&root).unwrap();
    }
    let refusal = std::env::var("PDO_TEST_EXTENSION_REFUSE")
        .map(|text| Refusal::parse(&text).unwrap())
        .unwrap_or_else(|_| Refusal::parse("project:update").unwrap());

    let config = pdo::Config::from_cli(Some(IpAddr::V4(Ipv4Addr::LOCALHOST)), port).unwrap();
    let (ext, _) = TestExtension::new(refusal.clone());
    let daemon = pdo::serve(config, ext.boxed()).await.unwrap();
    eprintln!(
        "demo daemon listening on http://{} (test extension refusing {refusal:?})",
        daemon.addr()
    );
    daemon.wait().await.unwrap();
}
