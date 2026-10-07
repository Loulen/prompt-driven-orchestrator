//! The Résumés de Run over SQLite (ADR-0084): the `run_summaries` table, one
//! JSON row per Run, and the mark that says which version of the Résumé and
//! which event of the log they were written up to.
//!
//! The event log's adapter (`sqlite_event_store`) writes a Run's Résumé in
//! the transaction of each append ([`write_in`]) and drops it in the one of a
//! forget; the boot rebuilds them all when the mark is not the log's
//! ([`replace`]); a read first catches up with the events another writer put
//! in the log while the daemon ran.

use anyhow::{Context, Result};
use sqlx::{Sqlite, SqliteConnection};
use tracing::{info, warn};

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
    summarize_in(&mut *conn, run_id).await?;
    // A mark left behind (events another writer put in the log) stays
    // behind: the next read catches up ([`catch_up`]); one of another version
    // waits for the boot's rebuild.
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

/// Write the Résumé of `run_id` from its log as `conn` sees it (drop it when
/// the log projects no Run).
async fn summarize_in(conn: &mut SqliteConnection, run_id: &str) -> Result<()> {
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
    Ok(())
}

/// Bring the Résumés up to the log's last event when events reached the log
/// outside an append of this daemon (a tool seeding the database of a running
/// instance): the Runs of the events past the mark get their Résumé rewritten,
/// then the mark moves to the last event. One indexed read when nothing was
/// missed. A mark of another version is left to the boot's rebuild.
async fn catch_up(db: &sqlx::SqlitePool) -> Result<()> {
    let behind: Option<(i64,)> = sqlx::query_as(
        "SELECT last_event_id FROM run_summaries_mark \
         WHERE id = 1 AND version = ? AND last_event_id < (SELECT COALESCE(MAX(id), 0) FROM events)",
    )
    .bind(SUMMARY_VERSION)
    .fetch_optional(db)
    .await
    .context("failed to read the run summaries mark")?;
    if behind.is_none() {
        return Ok(());
    }
    let mut tx = db.begin().await?;
    // The write comes first: it takes the write lock, so no append slips
    // between the read of the missed events and the move of the mark.
    let mark: Option<(i64,)> = sqlx::query_as(
        "UPDATE run_summaries_mark SET last_event_id = last_event_id \
         WHERE id = 1 AND version = ? RETURNING last_event_id",
    )
    .bind(SUMMARY_VERSION)
    .fetch_optional(&mut *tx)
    .await
    .context("failed to lock the run summaries mark")?;
    let Some((mark,)) = mark else {
        return Ok(());
    };
    let runs: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT run_id FROM events WHERE id > ?")
        .bind(mark)
        .fetch_all(&mut *tx)
        .await
        .context("failed to read the missed events")?;
    for (run_id,) in &runs {
        summarize_in(&mut tx, run_id).await?;
    }
    sqlx::query(
        "UPDATE run_summaries_mark SET last_event_id = (SELECT COALESCE(MAX(id), 0) FROM events) \
         WHERE id = 1",
    )
    .execute(&mut *tx)
    .await
    .context("failed to move the run summaries mark")?;
    tx.commit().await?;
    info!(
        "run summaries caught up with the event log ({} runs)",
        runs.len()
    );
    Ok(())
}

/// [`catch_up`], or the Résumés as they are when it fails (the log held by
/// another writer past the busy timeout): a read never fails for a write.
async fn catch_up_or_serve(db: &sqlx::SqlitePool) {
    if let Err(e) = catch_up(db).await {
        warn!("the run summaries could not catch up with the event log: {e:#}");
    }
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
    catch_up_or_serve(db).await;
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
    catch_up_or_serve(db).await;
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

/// Whether the mark is this version's and the log's last event, and every
/// Résumé has its log.
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
    // A Run forgotten by a daemon that did not keep the Résumés leaves one
    // behind, whatever the last event.
    let orphans: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM run_summaries WHERE run_id NOT IN (SELECT run_id FROM events)",
    )
    .fetch_one(db)
    .await
    .context("failed to check the run summaries")?;
    Ok(mark == Some((SUMMARY_VERSION, last)) && orphans == 0)
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
