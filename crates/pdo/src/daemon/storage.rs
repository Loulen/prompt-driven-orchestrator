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

    init_db(&db).await?;
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

pub(crate) async fn init_db(db: &sqlx::SqlitePool) -> Result<()> {
    // The event log and the tombstones of the forgotten Runs (ADR-0024), with
    // the Résumés de Run (ADR-0084), rebuilt from the log unless they are its
    // own — before the daemon serves anything.
    pdo_run::adapters::sqlite_event_store::init(db).await?;
    let events = pdo_run::adapters::sqlite_event_store::SqliteEventStore::new(db.clone());
    pdo_run::application::run_summary::ensure_summaries(&events)
        .await
        .context("failed to rebuild the run summaries")?;

    // The Triggers' tables belong to the trigger context (#927).
    pdo_trigger::adapters::sqlite::init(db)
        .await
        .context("failed to create trigger tables")?;

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

    // Materialised on demand — nothing seeded.
    pdo_workspace::adapters::sqlite::init(db)
        .await
        .context("failed to create project and provisioning rules tables")?;

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

// --- API handlers ---
