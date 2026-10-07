//! The Résumés de Run over SQLite (ADR-0084): the `run_summaries` table, one
//! JSON row per Run, and the mark that says which version of the Résumé and
//! which event of the log they were written up to.
//!
//! The event log's adapter (`sqlite_event_store`) writes a Run's Résumé in
//! the transaction of each append ([`write_in`]) and drops it in the one of a
//! forget; the boot rebuilds them all when the mark is not the log's
//! ([`replace`]).

use anyhow::{Context, Result};
use sqlx::{Sqlite, SqliteConnection};
use tracing::warn;

use crate::domain::run_summary::{summarize, RunSummary, SUMMARY_VERSION};

use super::sqlite_event_store::load_in;

/// Create the tables. Idempotent.
pub(crate) async fn init(db: &sqlx::SqlitePool) -> Result<()> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS run_summaries (
            run_id TEXT PRIMARY KEY,
            summary JSON NOT NULL
        )",
    )
    .execute(db)
    .await
    .context("failed to create run_summaries table")?;
    // One row: the version of the Résumés and the last event they cover.
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS run_summaries_mark (
            id INTEGER PRIMARY KEY CHECK (id = 1),
            version INTEGER NOT NULL,
            last_event_id INTEGER NOT NULL
        )",
    )
    .execute(db)
    .await
    .context("failed to create run_summaries_mark table")?;
    Ok(())
}

/// Write the Résumé of `run_id` from its log as `conn` sees it, and move the
/// mark to `event_id` when the Résumés are current up to the event before.
/// Called in the transaction of the append of `event_id`.
pub(crate) async fn write_in(
    conn: &mut SqliteConnection,
    run_id: &str,
    event_id: i64,
) -> Result<()> {
    let events = load_in(&mut *conn, run_id).await?;
    match summarize(&events) {
        Some(summary) => {
            sqlx::query("INSERT OR REPLACE INTO run_summaries (run_id, summary) VALUES (?, ?)")
                .bind(run_id)
                .bind(serde_json::to_string(&summary)?)
                .execute(&mut *conn)
                .await
                .context("failed to write the run summary")?;
        }
        None => drop_in(&mut *conn, run_id).await?,
    }
    // A mark left behind (by a daemon that did not keep the Résumés) stays
    // behind: the next boot rebuilds.
    sqlx::query(
        "UPDATE run_summaries_mark SET last_event_id = ? \
         WHERE id = 1 AND version = ? AND last_event_id = (SELECT COALESCE(MAX(id), 0) FROM events WHERE id < ?)",
    )
    .bind(event_id)
    .bind(SUMMARY_VERSION)
    .bind(event_id)
    .execute(&mut *conn)
    .await
    .context("failed to move the run summaries mark")?;
    Ok(())
}

/// Drop the Résumé of `run_id`, in the caller's transaction.
pub(crate) async fn drop_in(conn: &mut SqliteConnection, run_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM run_summaries WHERE run_id = ?")
        .bind(run_id)
        .execute(conn)
        .await
        .context("failed to drop the run summary")?;
    Ok(())
}

/// Every Résumé, newest first. An unreadable row is left out (logged).
pub(crate) async fn list(db: &sqlx::SqlitePool) -> Result<Vec<RunSummary>> {
    let rows: Vec<(String, String)> =
        sqlx::query_as("SELECT run_id, summary FROM run_summaries ORDER BY run_id DESC")
            .fetch_all(db)
            .await
            .context("failed to load the run summaries")?;
    Ok(rows
        .into_iter()
        .filter_map(|(run_id, summary)| parse(&run_id, &summary))
        .collect())
}

/// The Résumé of `run_id`.
pub(crate) async fn get(db: &sqlx::SqlitePool, run_id: &str) -> Result<Option<RunSummary>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT summary FROM run_summaries WHERE run_id = ?")
            .bind(run_id)
            .fetch_optional(db)
            .await
            .context("failed to load the run summary")?;
    Ok(row.and_then(|(summary,)| parse(run_id, &summary)))
}

fn parse(run_id: &str, summary: &str) -> Option<RunSummary> {
    serde_json::from_str(summary)
        .inspect_err(|e| warn!("the summary of run {run_id} is unreadable: {e}"))
        .ok()
}

/// Whether the mark is this version's and the log's last event.
pub(crate) async fn current(db: &sqlx::SqlitePool) -> Result<bool> {
    let mark: Option<(i64, i64)> =
        sqlx::query_as("SELECT version, last_event_id FROM run_summaries_mark WHERE id = 1")
            .fetch_optional(db)
            .await
            .context("failed to read the run summaries mark")?;
    let last: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) FROM events")
        .fetch_one(db)
        .await
        .context("failed to read the last event")?;
    Ok(mark == Some((SUMMARY_VERSION, last)))
}

/// Replace every Résumé and mark them current, in one transaction.
pub(crate) async fn replace(db: &sqlx::SqlitePool, summaries: Vec<RunSummary>) -> Result<()> {
    let mut tx = db.begin().await?;
    sqlx::query("DELETE FROM run_summaries")
        .execute(&mut *tx)
        .await?;
    for summary in &summaries {
        sqlx::query("INSERT OR REPLACE INTO run_summaries (run_id, summary) VALUES (?, ?)")
            .bind(&summary.run_id)
            .bind(serde_json::to_string(summary)?)
            .execute(&mut *tx)
            .await?;
    }
    mark_in(&mut tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn mark_in(tx: &mut sqlx::Transaction<'_, Sqlite>) -> Result<()> {
    sqlx::query(
        "INSERT OR REPLACE INTO run_summaries_mark (id, version, last_event_id) \
         VALUES (1, ?, (SELECT COALESCE(MAX(id), 0) FROM events))",
    )
    .bind(SUMMARY_VERSION)
    .execute(&mut **tx)
    .await
    .context("failed to mark the run summaries")?;
    Ok(())
}
