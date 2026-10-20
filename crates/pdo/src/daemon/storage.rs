//! The daemon's database: `pdo.db` under the repo root's `.pdo/`, every
//! context's tables, the Banque de skills' seed, and the one-shot fix-ups of
//! the pipeline stores that must run before the watcher attaches.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{error, info, warn};

use pdo_kernel::extension_host::ExtensionHost;
use pdo_skill::domain::extension_skill::ExtensionSkillSeed;
use pdo_skill::domain::seed::SeedOutcome;
use pdo_skill::domain::skill::ProvidedBy;

use crate::contexts::skill::skills;

/// Open `pdo.db` (created if absent) and every context's tables.
pub(super) async fn open(repo_root: &Path) -> Result<(sqlx::SqlitePool, std::path::PathBuf)> {
    let db_dir = repo_root.join(".pdo");
    std::fs::create_dir_all(&db_dir).context("failed to create .pdo directory")?;
    let db_path = db_dir.join("pdo.db");

    let db = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&format!("sqlite:{}?mode=rwc", db_path.display()))
        .await
        .context("failed to open SQLite database")?;

    init_db(&db, repo_root).await?;
    Ok((db, db_path))
}

/// The boot's fix-ups: the Banque de skills' seed (PDO's skills, then the
/// ones `host`'s extensions provide), the warning about the retired settings,
/// the stranded prompts and the legacy pipeline registries.
pub(super) async fn prepare(
    db: &sqlx::SqlitePool,
    db_path: &Path,
    repo_root: &Path,
    host: &dyn ExtensionHost,
) -> Result<()> {
    // #722: the seeded skill lands at every startup — created if absent, its
    // content rewritten when the built-in copy (versioned with PDO) differs.
    // A failed seed is logged, never fatal: the daemon must come up even when
    // the bank is in an unexpected state.
    //
    // A dedicated connection, closed as soon as the seed is done: the seed
    // must not leave its statements in the serving pool's caches.
    match sqlx::SqlitePool::connect(&format!("sqlite:{}?mode=rwc", db_path.display())).await {
        Ok(seed_db) => {
            let seeding = skills(
                &seed_db,
                repo_root,
                Arc::new(pdo_kernel::extension_host::NoExtensions),
            );
            match seeding.seed().await {
                Ok(outcomes) => {
                    for (id, outcome) in &outcomes {
                        match outcome {
                            SeedOutcome::Skipped { reason } => {
                                warn!("skill seed `{id}` skipped: {reason}")
                            }
                            _ => info!("skill seed `{id}`: {outcome:?}"),
                        }
                    }
                }
                Err(error) => error!("skill seed failed: {error}"),
            }
            seed_extension_skills(&seeding, host).await;
            seed_db.close().await;
        }
        Err(error) => error!("skill seed connection failed: {error}"),
    }

    // Must sit after the schema is up (the reader PRAGMA-probes the columns) and
    // before anything can consume a setting — the only moment where "one line per
    // boot" is a fact rather than a hope.
    pdo_settings::application::boot::warn_about_retired_settings(
        &pdo_settings::adapters::sqlite::SqliteInstanceConfig::new(db.clone()),
    )
    .await;

    // One-shot migration of prompts stranded in the flat
    // `<repo>/.pdo/pipelines/prompts/` dir into each pipeline's canonical
    // `<stem>.prompts/` dir. Must run BEFORE the file watcher attaches, or the
    // moves surface as spurious pipeline-modified events. Not gated on
    // `nested_daemon`: it is a pure filesystem fix-up, unrelated to tmux.
    {
        let repo_pipelines = repo_root.join(".pdo").join("pipelines");
        match pdo_pipeline::adapters::fs_migration::migrate_stranded_flat_prompts(&repo_pipelines) {
            Ok(n) if n > 0 => info!(
                "#231 migration: moved {n} stranded prompt(s) in {}",
                repo_pipelines.display()
            ),
            Ok(_) => {}
            Err(e) => warn!("#231 migration failed: {e}"),
        }
    }
    pdo_pipeline::adapters::registry::unify_legacy_registries(repo_root)
        .map_err(|error| anyhow::anyhow!("pipeline registry migration failed: {error}"))?;
    Ok(())
}

/// #33: the skills each extension provides land like PDO's own, rewritten when
/// the extension's version or their content changes. Logged, never fatal.
async fn seed_extension_skills(seeding: &pdo_skill::Skills, host: &dyn ExtensionHost) {
    for provided in host.skills() {
        let manifest = &provided.manifest;
        let seeds: Vec<ExtensionSkillSeed> = provided
            .skills
            .into_iter()
            .map(|skill| ExtensionSkillSeed {
                extension: ProvidedBy {
                    id: manifest.id.clone(),
                    version: manifest.version.clone(),
                },
                extension_name: manifest.name.clone(),
                name: skill.name,
                skill_md: skill.skill_md,
                files: skill
                    .files
                    .into_iter()
                    .map(|file| (file.path, file.content))
                    .collect(),
            })
            .collect();
        match seeding.seed_extension(&manifest.id, &seeds).await {
            Ok(outcomes) => {
                for (id, outcome) in &outcomes {
                    match outcome {
                        SeedOutcome::Skipped { reason } => {
                            warn!(
                                "skill `{id}` of extension `{}` skipped: {reason}",
                                manifest.id
                            )
                        }
                        _ => info!("skill `{id}` of extension `{}`: {outcome:?}", manifest.id),
                    }
                }
            }
            Err(error) => error!("skills of extension `{}` failed: {error}", manifest.id),
        }
    }
}

pub(crate) async fn init_db(db: &sqlx::SqlitePool, repo_root: &Path) -> Result<()> {
    // The event log and the tombstones of the forgotten Runs (ADR-0024), with
    // the Résumés de Run (ADR-0084), rebuilt from the log unless they are its
    // own — before the daemon serves anything.
    pdo_run::adapters::sqlite_event_store::init(db).await?;

    // The Projets and their members come before the Résumés: the one
    // migration of the Runs reads the members (ADR-0087).
    pdo_workspace::adapters::sqlite::init(db)
        .await
        .context("failed to create project and provisioning rules tables")?;
    let recorded =
        pdo_run::adapters::sqlite_event_store::record_legacy_projects(db, repo_root, |path| {
            let db = db.clone();
            async move {
                Ok(pdo_workspace::adapters::sqlite::first_project_containing(&db, &path).await?)
            }
        })
        .await
        .context("failed to record the project of the existing runs")?;
    let events = pdo_run::adapters::sqlite_event_store::SqliteEventStore::new(db.clone());
    if recorded > 0 {
        info!("project recorded on {recorded} existing runs (ADR-0087)");
        pdo_run::application::run_summary::rebuild_summaries(&events)
            .await
            .context("failed to rebuild the run summaries")?;
    } else {
        pdo_run::application::run_summary::ensure_summaries(&events)
            .await
            .context("failed to rebuild the run summaries")?;
    }

    // The Triggers' tables belong to the trigger context (#927).
    pdo_trigger::adapters::sqlite::init(db)
        .await
        .context("failed to create trigger tables")?;
    record_legacy_trigger_projects(db)
        .await
        .context("failed to record the project of the existing triggers")?;

    // The stored instance settings and the agentic profiles (seeded with the
    // reserved `Default`) belong to the settings context (#929).
    pdo_settings::adapters::sqlite::init(db)
        .await
        .context("failed to create the instance settings and agent profiles tables")?;

    // NO seed row: `minimal` and `full` are virtual defaults (ADR-0031 §2), so an
    // untouched install has zero rows here and still resolves both names.
    pdo_sandbox::adapters::sqlite_profiles::init(db)
        .await
        .context("failed to create sandbox_profiles table")?;

    // The audit journal (ADR-0044) and the page mounts belong to the platform
    // context, which creates its own tables (`Platform::init`).

    // The validations of the pipelines (CONTEXT.md « Pipeline validé »): no
    // row is every Projet, so nothing is seeded.
    pdo_pipeline::adapters::sqlite_validations::init(db)
        .await
        .context("failed to create pipeline validation tables")?;

    // The shared drafts of the pipelines (ADR-0088): one row per pipeline
    // someone opened.
    pdo_pipeline::adapters::sqlite_drafts::init(db)
        .await
        .context("failed to create pipeline draft table")?;

    // The versions a Save freezes (ADR-0088): one row per pipeline and number.
    pdo_pipeline::adapters::sqlite_versions::init(db)
        .await
        .context("failed to create pipeline version table")?;

    // The Banque de skills index (#668, ADR-0062). Content lives on disk under
    // `<repo_root>/.pdo/skills/<id>/`; nothing is seeded.
    pdo_skill::adapters::sqlite::init(db)
        .await
        .context("failed to create skill bank tables")?;

    // Stats absorptions (ADR-0077): instance configuration, nothing seeded.
    pdo_stats::adapters::sqlite_absorptions::init(db)
        .await
        .context("failed to create stats_absorptions table")?;

    Ok(())
}

/// ADR-0087's one migration of the existing Triggers: each one still pending
/// receives the Projet the rule before ADR-0087 gave its Runs — the first
/// Projet holding its target repository, or another spelling of it (as the
/// Runs' migration matches) — else none.
async fn record_legacy_trigger_projects(db: &sqlx::SqlitePool) -> Result<()> {
    let pending = pdo_trigger::adapters::sqlite::legacy_project_pending(db).await?;
    for (id, target_repo) in &pending {
        let mut project = None;
        if let Some(repo) = target_repo.as_deref().filter(|r| !r.trim().is_empty()) {
            for spelling in pdo_kernel::repository_path::member_spellings(repo) {
                project = pdo_workspace::adapters::sqlite::first_project_containing(db, &spelling)
                    .await?;
                if project.is_some() {
                    break;
                }
            }
        }
        pdo_trigger::adapters::sqlite::record_legacy_project(db, id, project.as_deref()).await?;
    }
    if !pending.is_empty() {
        info!(
            "project recorded on {} existing triggers (ADR-0087)",
            pending.len()
        );
    }
    Ok(())
}

// --- API handlers ---

#[cfg(test)]
mod tests {
    use super::*;

    async fn memory_db() -> sqlx::SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    /// The database a daemon before ADR-0087 left: one Projet per path, and
    /// `run_started` events recording no Projet.
    async fn legacy(db: &sqlx::SqlitePool) {
        for sql in [
            "CREATE TABLE projects (id TEXT PRIMARY KEY, name TEXT NOT NULL, harness TEXT, created_at TEXT NOT NULL)",
            "CREATE TABLE project_members (path TEXT PRIMARY KEY, project_id TEXT NOT NULL, created_at TEXT NOT NULL)",
            "INSERT INTO projects (id, name, created_at) VALUES ('p1', 'Alpha', 's')",
        ] {
            sqlx::query(sql).execute(db).await.unwrap();
        }
        sqlx::query(
            "INSERT INTO project_members (path, project_id, created_at) VALUES ('/repos/web', 'p1', 's'), ('/root', 'p1', 's')",
        )
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "CREATE TABLE triggers (id TEXT PRIMARY KEY, name TEXT NOT NULL, pipeline_id TEXT NOT NULL, \
             pipeline_name TEXT NOT NULL DEFAULT '', target_repo TEXT, target_repos TEXT, \
             source_branch TEXT, input_template TEXT NOT NULL DEFAULT '', \
             variables JSON NOT NULL DEFAULT '{}', cron TEXT NOT NULL, guard_command TEXT, \
             overlap_policy TEXT NOT NULL DEFAULT 'skip', max_concurrent INTEGER, sandbox TEXT, \
             enabled INTEGER NOT NULL DEFAULT 1, next_fire_at TEXT, last_fired_at TEXT, \
             last_outcome TEXT, created_at TEXT NOT NULL)",
        )
        .execute(db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO triggers (id, name, pipeline_id, target_repo, cron, created_at) VALUES \
             ('t-web', 'web', 'p', '/repos/web/.', '0 3 * * *', 's'), \
             ('t-loose', 'loose', 'p', '/repos/loose', '0 3 * * *', 's')",
        )
        .execute(db)
        .await
        .unwrap();
        pdo_run::adapters::sqlite_event_store::init(db)
            .await
            .unwrap();
        for (run, payload) in [
            (
                "r-web",
                r#"{"pipeline_name":"p","target_repo":"/repos/web/"}"#,
            ),
            (
                "r-loose",
                r#"{"pipeline_name":"p","target_repo":"/repos/loose"}"#,
            ),
            ("r-root", r#"{"pipeline_name":"p"}"#),
            (
                "r-new",
                r#"{"pipeline_name":"p","target_repo":"/repos/web","project_id":null}"#,
            ),
        ] {
            sqlx::query(
                "INSERT INTO events (run_id, ts, kind, payload) VALUES (?, '2026-10-01T00:00:00Z', 'run_started', ?)",
            )
            .bind(run)
            .bind(payload)
            .execute(db)
            .await
            .unwrap();
        }
    }

    async fn recorded(db: &sqlx::SqlitePool, run: &str) -> Option<serde_json::Value> {
        let (payload,): (String,) =
            sqlx::query_as("SELECT payload FROM events WHERE run_id = ? AND kind = 'run_started'")
                .bind(run)
                .fetch_one(db)
                .await
                .unwrap();
        let payload: serde_json::Value = serde_json::from_str(&payload).unwrap();
        payload.get("project_id").cloned()
    }

    #[tokio::test]
    async fn the_existing_runs_get_once_the_projet_the_old_rule_gave_them() {
        let db = memory_db().await;
        legacy(&db).await;

        init_db(&db, Path::new("/root")).await.unwrap();

        assert_eq!(recorded(&db, "r-web").await, Some(serde_json::json!("p1")));
        assert_eq!(
            recorded(&db, "r-loose").await,
            Some(serde_json::Value::Null)
        );
        // No target: the daemon's repo root, which Alpha holds.
        assert_eq!(recorded(&db, "r-root").await, Some(serde_json::json!("p1")));
        // Already recorded (in none): never revisited.
        assert_eq!(recorded(&db, "r-new").await, Some(serde_json::Value::Null));
        // The Résumés carry it.
        let store = pdo_run::adapters::sqlite_event_store::SqliteEventStore::new(db.clone());
        use pdo_run::application::ports::EventStore;
        let web = store.summary("r-web").await.unwrap().unwrap();
        assert_eq!(web.project_id.as_deref(), Some("p1"));
        let loose = store.summary("r-loose").await.unwrap().unwrap();
        assert_eq!(loose.project_id, None);

        // Once: a later membership moves no Run.
        sqlx::query(
            "INSERT INTO project_members (path, project_id, created_at) VALUES ('/repos/loose', 'p1', 's')",
        )
        .execute(&db)
        .await
        .unwrap();
        init_db(&db, Path::new("/root")).await.unwrap();
        assert_eq!(
            recorded(&db, "r-loose").await,
            Some(serde_json::Value::Null)
        );
    }

    #[tokio::test]
    async fn the_existing_triggers_get_once_the_projet_the_old_rule_gave_them() {
        let db = memory_db().await;
        legacy(&db).await;
        init_db(&db, Path::new("/root")).await.unwrap();
        let project = |id: &'static str| {
            let db = db.clone();
            async move {
                pdo_trigger::adapters::sqlite::get(&db, id)
                    .await
                    .unwrap()
                    .unwrap()
                    .project_id
            }
        };
        assert_eq!(
            project("t-web").await.as_deref(),
            Some("p1"),
            "`/repos/web/.` is `/repos/web`"
        );
        assert_eq!(project("t-loose").await, None);
        sqlx::query(
            "INSERT INTO project_members (path, project_id, created_at) VALUES ('/repos/loose', 'p1', 's')",
        )
        .execute(&db)
        .await
        .unwrap();
        init_db(&db, Path::new("/root")).await.unwrap();
        assert_eq!(project("t-loose").await, None, "once");
    }
}
