//! The staging profiles in SQLite (`sandbox_profiles` of `pdo.db`): the one
//! implementation of [`SandboxProfileRepository`].

use std::collections::BTreeMap;

use sqlx::{Row, SqlitePool};

use crate::application::ports::{BoxFuture, SandboxProfileRepository, StoreError};
use crate::domain::image::ProfileImage;
use crate::domain::profile::draft::ProfileDraft;
use crate::domain::profile::ProfileDiff;

/// The profiles of one database.
#[derive(Debug, Clone)]
pub struct SqliteProfiles {
    db: SqlitePool,
}

impl SqliteProfiles {
    pub fn new(db: SqlitePool) -> Self {
        Self { db }
    }
}

fn store_error(e: sqlx::Error) -> StoreError {
    StoreError(e.to_string())
}

/// Create the `sandbox_profiles` table if absent, then apply the additive column
/// migrations.
///
/// The table was brand-new in #432, so `CREATE TABLE IF NOT EXISTS` was the whole
/// migration then. **#468 was the first `ALTER`**, and it used the idempotent PRAGMA-guarded
/// `ALTER TABLE … ADD COLUMN` idiom (precedent: `max_concurrent`, #239 in
/// the Triggers' store), **never** a migration runner. #467 adds the second, by the
/// same idiom and for the same reasons — two divergent mechanics for two fields of the same
/// profile would be the worst possible outcome. The `CREATE` above carries both columns too,
/// so a fresh database takes the short path and an existing one — where the `CREATE` is a
/// no-op — takes the `ALTER`s. Both end on the same schema; that redundancy is the point of
/// the idiom, not an oversight. The idiom being idempotent *per column*, the order in which
/// the two landed does not matter.
///
/// **No seed row**: `minimal` and `full` are virtual (ADR-0031 §2).
///
/// `COLLATE NOCASE` is belt-and-braces — [`validate_profile_name`] makes a case
/// collision unreachable, but if a validator is ever loosened the index refuses the
/// duplicate instead of creating two profiles that display identically.
pub async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sandbox_profiles (
            name       TEXT PRIMARY KEY COLLATE NOCASE,
            disabled   JSON NOT NULL DEFAULT '[]',
            extras     JSON NOT NULL DEFAULT '[]',
            env        JSON NOT NULL DEFAULT '{}',
            image      JSON,
            updated_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // Additive migration (#468): per-profile environment variables. A pre-#468
    // `~/.pdo/pdo.db` got the table via `CREATE TABLE IF NOT EXISTS` — a no-op there — so
    // the column must be added out-of-band or every `SELECT … env` below would fail. The
    // PRAGMA guard keeps it idempotent (a bare `ALTER … ADD COLUMN` errors "duplicate
    // column name" on an already-migrated DB) and is preferred over swallowing the error
    // blindly, which would hide a genuine failure. `DEFAULT '{}'` so existing rows read
    // back as "no env", which is exactly what they had.
    add_column_if_absent(db, "env", "JSON NOT NULL DEFAULT '{}'").await?;
    // Additive migration (#467): the per-profile image source. NULLABLE and with **no**
    // default, unlike `env`: "poses nothing" is a first-class state here (the instance-wide
    // setting decides), so `NULL` is the honest empty value rather than a sentinel `'null'`
    // string every reader would have to know about. Existing rows read back as `None`, which
    // is exactly the behaviour they had.
    add_column_if_absent(db, "image", "JSON").await?;
    Ok(())
}

/// The PRAGMA-guarded `ALTER TABLE … ADD COLUMN` idiom, once (#467). Extracted the moment
/// there were two of them: the guard is the load-bearing half (a bare `ALTER` errors
/// "duplicate column name" on an already-migrated DB, and swallowing that error blindly would
/// hide a genuine failure), and it must not be re-typed per column.
async fn add_column_if_absent(
    db: &SqlitePool,
    column: &str,
    decl: &str,
) -> Result<(), sqlx::Error> {
    let present = sqlx::query("SELECT 1 FROM pragma_table_info('sandbox_profiles') WHERE name = ?")
        .bind(column)
        .fetch_optional(db)
        .await?
        .is_some();
    if !present {
        // `column`/`decl` are compile-time literals from this module, never user input — the
        // only reason they are formatted in rather than bound is that SQLite has no bind
        // parameter for an identifier or a type declaration.
        sqlx::query(&format!(
            "ALTER TABLE sandbox_profiles ADD COLUMN {column} {decl}"
        ))
        .execute(db)
        .await?;
    }
    Ok(())
}

/// Four columns rather than one `diff` blob: they validate differently, two of them play
/// opposite roles at resolution (subtract vs add) while the other two are not diffs at all
/// (#468, #467), and a corrupt one degrades to its empty value **on its own** — a shared blob
/// would take the whole profile down with it. Precedent: `variables JSON` in
/// the Triggers' store.
fn row_to_diff(row: &sqlx::sqlite::SqliteRow) -> ProfileDiff {
    let disabled: String = row.get("disabled");
    let extras: String = row.get("extras");
    let env: String = row.get("env");
    // #467: NULL (poses nothing) and an unreadable value both degrade to `None`, i.e. "the
    // instance-wide setting decides". That is safe HERE and only here: this is the *live* read
    // for the editor and for the create-run resolve, where falling back to the instance setting
    // is a legitimate answer. The Run's FROZEN copy has no such latitude — `frozen::
    // frozen_image` fails the Run loud, because there "fall back" would mean silently starting a
    // container in a DIFFERENT image than the nodes that already launched saw.
    let image: Option<String> = row.get("image");
    ProfileDiff {
        name: row.get("name"),
        disabled: serde_json::from_str(&disabled).unwrap_or_default(),
        extras: serde_json::from_str(&extras).unwrap_or_default(),
        env: serde_json::from_str(&env).unwrap_or_default(),
        image: image.as_deref().and_then(|s| serde_json::from_str(s).ok()),
        updated_at: row.get("updated_at"),
    }
}

/// The stored diff for `name`, or `None` when the profile has never been edited.
async fn get_diff(db: &SqlitePool, name: &str) -> Result<Option<ProfileDiff>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT name, disabled, extras, env, image, updated_at \
         FROM sandbox_profiles WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(db)
    .await?;
    Ok(row.as_ref().map(row_to_diff))
}

/// Every materialised diff, by name.
async fn list_diffs(db: &SqlitePool) -> Result<Vec<ProfileDiff>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT name, disabled, extras, env, image, updated_at \
         FROM sandbox_profiles ORDER BY name",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(row_to_diff).collect())
}
/// `upsert`, not create-then-update: the caller cannot know whether `full` is already
/// materialised, and ADR-0031 §2 says editing it *is* what materialises it.
async fn upsert(
    db: &SqlitePool,
    name: &str,
    disabled: &[String],
    extras: &[String],
    env: &BTreeMap<String, String>,
    image: Option<&ProfileImage>,
) -> Result<ProfileDiff, sqlx::Error> {
    let now = pdo_kernel::clock::now_iso();
    let disabled_json = serde_json::to_string(disabled).unwrap_or_else(|_| "[]".to_string());
    let extras_json = serde_json::to_string(extras).unwrap_or_else(|_| "[]".to_string());
    let env_json = serde_json::to_string(env).unwrap_or_else(|_| "{}".to_string());
    // `None` binds SQL NULL, which is how "poses nothing" is stored (#467) — and, `image` being
    // a FULL replacement like every other field here, it is also how clearing one works.
    let image_json: Option<String> = image.and_then(|i| serde_json::to_string(i).ok());
    sqlx::query(
        "INSERT INTO sandbox_profiles (name, disabled, extras, env, image, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET
            disabled = excluded.disabled,
            extras = excluded.extras,
            env = excluded.env,
            image = excluded.image,
            updated_at = excluded.updated_at",
    )
    .bind(name)
    .bind(&disabled_json)
    .bind(&extras_json)
    .bind(&env_json)
    .bind(&image_json)
    .bind(&now)
    .execute(db)
    .await?;
    get_diff(db, name).await?.ok_or(sqlx::Error::RowNotFound)
}

/// Delete a materialised row. `false` when there was none (a virtual default that was
/// never edited, or an unknown name) — the caller turns that into a 404.
///
/// Unconditional by design (ADR-0031 §7: *soft* guard-rail, no referential integrity in
/// the database). The referents dialog exists precisely because this does not check.
async fn delete(db: &SqlitePool, name: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM sandbox_profiles WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

impl SandboxProfileRepository for SqliteProfiles {
    fn get<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Option<ProfileDiff>, StoreError>> {
        Box::pin(async move { get_diff(&self.db, name).await.map_err(store_error) })
    }

    fn list(&self) -> BoxFuture<'_, Result<Vec<ProfileDiff>, StoreError>> {
        Box::pin(async move { list_diffs(&self.db).await.map_err(store_error) })
    }

    fn upsert<'a>(
        &'a self,
        name: &'a str,
        profile: &'a ProfileDraft,
    ) -> BoxFuture<'a, Result<ProfileDiff, StoreError>> {
        Box::pin(async move {
            upsert(
                &self.db,
                name,
                &profile.disabled,
                &profile.extras,
                &profile.env,
                profile.image.as_ref(),
            )
            .await
            .map_err(store_error)
        })
    }

    fn delete<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<bool, StoreError>> {
        Box::pin(async move { delete(&self.db, name).await.map_err(store_error) })
    }
}

/// Stage 3 (ADR-0080): the adapter against a real (in-memory) SQLite database.
#[cfg(test)]
mod tests {
    use super::*;

    /// One connection: every connection to `sqlite::memory:` is its own database.
    async fn memory() -> SqlitePool {
        sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap()
    }

    async fn store() -> SqliteProfiles {
        let db = memory().await;
        init(&db).await.unwrap();
        SqliteProfiles::new(db)
    }

    fn draft() -> ProfileDraft {
        ProfileDraft {
            disabled: vec![".claude/plugins".into()],
            extras: vec![".gitconfig".into()],
            env: BTreeMap::from([("FOO".to_string(), "bar".to_string())]),
            image: Some(ProfileImage::Registry {
                image_ref: "acme/agent:1".into(),
            }),
        }
    }

    #[tokio::test]
    async fn an_upsert_round_trips_every_column() {
        let store = store().await;
        let stored = store.upsert("team", &draft()).await.unwrap();
        assert_eq!(stored.name, "team");
        assert_eq!(stored.disabled, vec![".claude/plugins"]);
        assert_eq!(stored.extras, vec![".gitconfig"]);
        assert_eq!(stored.env["FOO"], "bar");
        assert_eq!(stored.image, draft().image);
        assert!(stored.updated_at.ends_with('Z'), "{}", stored.updated_at);
        assert_eq!(store.get("team").await.unwrap(), Some(stored));
    }

    #[tokio::test]
    async fn an_upsert_replaces_the_whole_row_and_clears_the_image() {
        let store = store().await;
        store.upsert("team", &draft()).await.unwrap();
        let cleared = store
            .upsert("team", &ProfileDraft::default())
            .await
            .unwrap();
        assert!(cleared.disabled.is_empty() && cleared.extras.is_empty());
        assert!(cleared.env.is_empty() && cleared.image.is_none());
        assert_eq!(store.list().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn names_are_case_insensitive_keys_and_delete_reports_a_missing_row() {
        let store = store().await;
        store.upsert("team", &draft()).await.unwrap();
        assert!(store.get("TEAM").await.unwrap().is_some());
        assert!(store.delete("team").await.unwrap());
        assert!(!store.delete("team").await.unwrap());
        assert_eq!(store.get("team").await.unwrap(), None);
    }

    /// A pre-#468 table (no `env`, no `image`) is migrated in place, and its rows read
    /// back as "no env, no image".
    #[tokio::test]
    async fn init_migrates_an_old_table_additively() {
        let db = memory().await;
        sqlx::query(
            "CREATE TABLE sandbox_profiles (
                name TEXT PRIMARY KEY COLLATE NOCASE,
                disabled JSON NOT NULL DEFAULT '[]',
                extras JSON NOT NULL DEFAULT '[]',
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO sandbox_profiles (name, updated_at) VALUES ('old', 't')")
            .execute(&db)
            .await
            .unwrap();
        init(&db).await.unwrap();
        init(&db).await.unwrap();
        let old = get_diff(&db, "old").await.unwrap().unwrap();
        assert!(old.env.is_empty() && old.image.is_none());
    }

    /// A corrupt column degrades to its empty value on its own.
    #[tokio::test]
    async fn a_corrupt_column_degrades_alone() {
        let store = store().await;
        store.upsert("team", &draft()).await.unwrap();
        sqlx::query("UPDATE sandbox_profiles SET image = 'not json', extras = '{'")
            .execute(&store.db)
            .await
            .unwrap();
        let row = store.get("team").await.unwrap().unwrap();
        assert!(row.image.is_none() && row.extras.is_empty());
        assert_eq!(row.env["FOO"], "bar");
    }
}
