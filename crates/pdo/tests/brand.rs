//! Layer 3 — the Marque of the binary.
//!
//! A binary declares its Marque at compile time, with `Config::with_brand`, next
//! to its release channel (ADR draft « la Marque est déclarée par le binaire à
//! la compilation »). Each daemon here goes through `serve`, as such a binary
//! would, under a tempdir.
//!
//! What is proven here:
//!   1. the vanilla binary is PDO: the page it serves is titled `PDO` and carries
//!      `data-brand="pdo"` (what the front reads before its first render), and
//!      its logos under `/shared/brand/` are PDO's;
//!   2. a `Config` that declares Acogni Driver serves the same page titled
//!      `Acogni Driver`, `data-brand="acogni-driver"`, with Acogni Driver's logos;
//!   3. the Marque changes nothing else: the release channel is still the
//!      vanilla one, named after the core.

use std::net::SocketAddr;
use std::path::Path;

use pdo::{serve, Brand, Config, Daemon, DaemonConfig};

fn quiet(home: &Path) -> DaemonConfig {
    std::fs::create_dir_all(home).unwrap();
    DaemonConfig {
        sandbox_home_override: Some(home.to_path_buf()),
        tmux_cmd_override: Some("exec sleep 600".to_string()),
        run_update_check_loop: false,
        run_trigger_scheduler_loop: false,
        price_refresh_at_boot: false,
        nested_daemon: true,
        ..DaemonConfig::default()
    }
}

/// Start a daemon through `serve` under `root`, declaring `brand` (none: the vanilla one).
async fn start(root: &Path, brand: Option<Brand>) -> (Daemon, String) {
    let repo = root.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let mut config = Config::new(SocketAddr::from(([127, 0, 0, 1], 0)), &repo)
        .with_daemon_config(quiet(&root.join("home")));
    if let Some(brand) = brand {
        config = config.with_brand(brand);
    }
    let daemon = serve(config, vec![]).await.unwrap();
    let base = format!("http://{}", daemon.addr());
    (daemon, base)
}

async fn get(base: &str, path: &str) -> (u16, String, String) {
    let resp = reqwest::get(format!("{base}{path}")).await.unwrap();
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .map(|v| v.to_str().unwrap().to_string())
        .unwrap_or_default();
    (status, content_type, resp.text().await.unwrap())
}

/// The page and the three logos of the daemon at `base` say `name`, `id`.
async fn assert_brand(base: &str, name: &str, id: &str) {
    // The page, at the root and on a deep link of the front alike.
    for path in ["/", "/runs/some-run/review"] {
        let (status, _, page) = get(base, path).await;
        assert_eq!(status, 200, "{path}");
        assert!(
            page.contains(&format!("<title>{name}</title>")),
            "{path}: {page}"
        );
        assert!(
            page.contains(&format!("data-brand=\"{id}\"")),
            "{path}: {page}"
        );
    }
    for logo in ["mark.svg", "lockup.svg", "favicon.svg"] {
        let (status, content_type, svg) = get(base, &format!("/shared/brand/{logo}")).await;
        assert_eq!(status, 200, "{logo}");
        assert_eq!(content_type, "image/svg+xml", "{logo}");
        assert!(svg.starts_with("<svg"), "{logo}: {svg}");
        if logo != "favicon.svg" {
            assert!(
                svg.contains(&format!("aria-label=\"{name}\"")),
                "{logo}: {svg}"
            );
        }
    }
}

#[tokio::test]
async fn the_vanilla_binary_is_pdo() {
    let root = tempfile::tempdir().unwrap();
    let (_daemon, base) = start(root.path(), None).await;
    assert_brand(&base, "PDO", "pdo").await;
    let (_, _, page) = get(&base, "/").await;
    assert!(!page.contains("Acogni Driver"), "{page}");
}

#[tokio::test]
async fn a_config_that_declares_acogni_driver_is_acogni_driver() {
    let root = tempfile::tempdir().unwrap();
    let (_daemon, base) = start(root.path(), Some(Brand::AcogniDriver)).await;
    assert_brand(&base, "Acogni Driver", "acogni-driver").await;
    // The logos differ from the vanilla ones.
    let vanilla_root = tempfile::tempdir().unwrap();
    let (_vanilla, vanilla_base) = start(vanilla_root.path(), Some(Brand::Pdo)).await;
    let (_, _, mark) = get(&base, "/shared/brand/mark.svg").await;
    let (_, _, vanilla_mark) = get(&vanilla_base, "/shared/brand/mark.svg").await;
    assert_ne!(mark, vanilla_mark);
}

#[tokio::test]
async fn the_brand_leaves_the_release_channel_alone() {
    let _env = crate::common::lock_update_check_unset();
    let root = tempfile::tempdir().unwrap();
    let (_daemon, base) = start(root.path(), Some(Brand::AcogniDriver)).await;
    let body: serde_json::Value = reqwest::get(format!("{base}/update"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body["channel_name"], "PDO", "{body}");
    assert_eq!(body["installed_version"], env!("CARGO_PKG_VERSION"));
}
