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
//! - `PDO_TEST_EXTENSION_INCOMPATIBLE`: `1` starts with the UI bundle declaring
//!   a React major the front does not run (`POST /ext/test/incompatible` switches
//!   it while the daemon runs).
//! - `PDO_TEST_INTEROP`: `1` also loads the three interop peers `a`, `b`, `c`
//!   (`test_extension/interop.rs`), which mutate their items through the Host:
//!   `POST /ext/a/items {"name": …}` creates one, `GET /ext/a/items` lists
//!   them, `POST /ext/b/refusal {"refuses": true}` has `b` refuse every
//!   Operation on an extension's resource.
//! - `PDO_DEMO_HOSTED`: which of the hosted-location test extensions
//!   (`test_extension/hosted.rs`) to add after the test extension: `a,b` (A
//!   hosts `a/panneau`, B contributes), `a` (B removed), none by default.
//! - `PDO_DEMO_B_PANELS`: how many sections B contributes to `a/panneau`, `1`
//!   by default (`POST /ext/b/panels {"count": n}` switches it while the
//!   daemon runs). Its ids are the interop peers': not with `PDO_TEST_INTEROP`.
//!
//! - `PDO_TEST_NODE_ENV`: the variables the test extension adds to every Node
//!   (API 0.8), `NAME=value` pairs separated by commas (`FIXTURE_HELLO=monde`);
//!   none by default (`PUT /ext/test/launch {"env": {…}}` switches them).
//! - `PDO_TEST_NODE_PREAMBLE`: the text it puts at the top of every agent's
//!   preamble (`Ligne de fixture`); none by default (`PUT /ext/test/launch
//!   {"preamble": …}`).
//!
//! It suspends a Trigger on `PUT /ext/test/suspensions {"trigger_id", "reason"}`
//! and lifts it on `DELETE /ext/test/suspensions/<id>`.
//!
//! The test extension keeps its data under `$HOME/.pdo/extensions/test/`: give
//! the demo daemon a disposable `HOME`.
//!
//! With `--exact`, the filter is the full path `demo_daemon::demo_daemon`.
//!
//! The caller named by the `x-pdo-test-actor` header is the identified Actor.
//!
//! It plays the `fixture` of the core's Feature Paths (CORE-3): it declares the
//! Ressource `test/note` (`GET /resources` lists it after the core's, and the
//! interop peers' `<id>/item` with `PDO_TEST_INTEROP=1`), and
//! `GET /ext/test/operations` lists every Operation it was asked about, with
//! its actor and its Projets (`operation.projects`).
//!
//! A second test extension (`test2`) is registered after it: it adds a section
//! to `project/edit`, so that location shows two extensions' sections.
//!
//! Its command line answers `PDO_DAEMON_URL=http://127.0.0.1:<port> pdo ext test
//! echo salut` (stdout `salut`, stderr the Actor it sees), and its skill
//! `test-extension-cli` is in the Banque de skills, in the « Test extension »
//! folder.

use std::net::{IpAddr, Ipv4Addr};

use crate::test_extension::hosted::{ContributorB, HostA};
use crate::test_extension::interop::Peer;
use crate::test_extension::{Refusal, SecondTestExtension, TestExtension};

#[tokio::test(flavor = "multi_thread")]
#[ignore = "serves until killed: the demo daemon of the Feature Paths"]
async fn demo_daemon() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
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
    let (ext, control) = TestExtension::new(refusal.clone());
    control
        .set_incompatible(std::env::var("PDO_TEST_EXTENSION_INCOMPATIBLE").as_deref() == Ok("1"));
    if let Ok(vars) = std::env::var("PDO_TEST_NODE_ENV") {
        control.launch.set_env(
            vars.split(',')
                .filter_map(|pair| pair.split_once('='))
                .map(|(k, v)| (k.trim().to_string(), v.to_string()))
                .collect(),
        );
    }
    if let Ok(text) = std::env::var("PDO_TEST_NODE_PREAMBLE") {
        control.launch.set_preamble(Some(text));
    }
    let mut extensions = ext.boxed();
    extensions.push(Box::new(SecondTestExtension));
    let hosted = std::env::var("PDO_DEMO_HOSTED").unwrap_or_default();
    let interop = std::env::var("PDO_TEST_INTEROP").as_deref() == Ok("1");
    assert!(
        !(interop && !hosted.trim().is_empty()),
        "PDO_TEST_INTEROP and PDO_DEMO_HOSTED both load extensions `a` and `b`: pick one"
    );
    for id in hosted.split(',').map(str::trim).filter(|id| !id.is_empty()) {
        match id {
            "a" => extensions.push(Box::new(HostA)),
            "b" => {
                let (b, panels) = ContributorB::new();
                if let Ok(count) = std::env::var("PDO_DEMO_B_PANELS") {
                    panels.set_panels(count.parse().expect("PDO_DEMO_B_PANELS is a number"));
                }
                extensions.push(Box::new(b));
            }
            other => panic!("PDO_DEMO_HOSTED: unknown extension `{other}` (a, b)"),
        }
    }
    if interop {
        extensions.extend(Peer::trio().0);
    }
    let daemon = pdo::serve(config, extensions).await.unwrap();
    eprintln!(
        "demo daemon listening on http://{} (test extension refusing {refusal:?})",
        daemon.addr()
    );
    daemon.wait().await.unwrap();
}
