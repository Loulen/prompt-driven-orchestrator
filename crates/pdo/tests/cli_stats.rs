//! Layer 3a — `pdo stats` through the real binary, against a real daemon: each
//! tab read over the surface's period presets, the figures the daemon folds
//! (the same window the surface reads, so the same numbers), and the
//! absorptions combined and uncombined.

use crate::cli_operator::pdo;
use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

/// The 7-day window of the surface, today included, as `presetPeriod` builds it.
fn seven_days() -> (String, String) {
    let today = chrono::Utc::now().date_naive();
    let day = |d: chrono::NaiveDate| format!("{d}T00:00:00.000Z");
    (
        day(today - chrono::Duration::days(6)),
        day(today + chrono::Duration::days(1)),
    )
}

#[tokio::test]
async fn every_tab_reads_the_window_of_the_surface() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (from, to) = seven_days();
    let surface = daemon
        .get_json(&format!(
            "/stats/overview?from={from}&to={to}&bucket=day&completed_only=false"
        ))
        .await;
    let cli = pdo(&daemon, &["stats", "runs", "--period", "7d", "--json"])
        .await
        .json();
    assert_eq!(cli, surface, "the CLI reads the surface's window");
    assert_eq!(
        pdo(&daemon, &["stats", "runs"]).await.ok(),
        "runs 0 · failed 0 · failure rate — · busiest 0\n"
    );
    assert!(pdo(&daemon, &["stats", "sessions", "--period", "30d"])
        .await
        .ok()
        .starts_with("sessions 0\n"));
    assert!(pdo(&daemon, &["stats", "triggers", "--period", "all"])
        .await
        .ok()
        .starts_with("fires that created a run 0 · triggers fired 0 of 0 enabled · fires 0\n"));
    let cost = pdo(&daemon, &["stats", "cost", "--period", "90d"]).await;
    assert!(cost.ok().starts_with("total "), "{}", cost.stdout);
    let performance = pdo(&daemon, &["stats", "performance", "--json"])
        .await
        .json();
    assert!(performance["by_pipeline"].is_array(), "{performance}");
    let window = pdo(
        &daemon,
        &[
            "stats",
            "runs",
            "--from",
            "2026-01-01",
            "--to",
            "2026-02-01",
            "--bucket",
            "week",
        ],
    )
    .await;
    window.ok();
    let bad = pdo(
        &daemon,
        &["stats", "runs", "--from", "yesterday", "--to", "today"],
    )
    .await;
    assert_eq!(bad.code, Some(1), "{}", bad.stderr);
}

#[tokio::test]
async fn rows_are_combined_and_uncombined() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    assert_eq!(
        pdo(&daemon, &["stats", "absorptions"]).await.ok(),
        "no absorption\n"
    );
    let combined = pdo(
        &daemon,
        &[
            "stats",
            "combine",
            "--dimension",
            "pipeline",
            "--into",
            "ship=Ship",
            "--member",
            "ship-old=Ship (old)",
        ],
    )
    .await;
    assert_eq!(combined.ok(), "pipeline\tShip\t<- Ship (old)\n");
    let list = daemon.get_json("/stats/absorptions").await;
    assert_eq!(list["absorptions"][0]["absorbent"]["key"], "ship", "{list}");
    let uncombined = pdo(
        &daemon,
        &[
            "stats",
            "uncombine",
            "--dimension",
            "pipeline",
            "--member",
            "ship-old",
        ],
    )
    .await;
    assert_eq!(uncombined.ok(), "no absorption\n");
    let missing = pdo(
        &daemon,
        &[
            "stats",
            "uncombine",
            "--dimension",
            "pipeline",
            "--member",
            "ship-old",
        ],
    )
    .await;
    assert_eq!(missing.code, Some(4), "{}", missing.stderr);
}
