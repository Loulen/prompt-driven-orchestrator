//! Journal d'audit hors-Run (#507, ADR-0044). Le troisième journal de PDO,
//! frère de [`crate::trigger_store`] / [`crate::instance_config`] : il consigne
//! les mutations de configuration faites *hors Run* — désactiver un Trigger à la
//! main, éditer son cron, mettre les Triggers en pause — invisibles à
//! l'`event_log` (dont l'`Event.run_id` est obligatoire, la projection refusant
//! tout fragment sans `RunStarted`). Append-only, sans `run_id`, sur le pool
//! `pdo.db` partagé.
//!
//! **Invariant (ADR-0044) :** l'audit peut *sous-rapporter* (mutation réussie,
//! ligne manquante sur disque plein / base verrouillée) mais **jamais**
//! *sur-rapporter* (une ligne pour une mutation non commitée). D'où l'ordre au
//! seam : lire l'avant → muter → écrire l'audit **après le commit**, en
//! best-effort ([`record_best_effort`]). Un échec d'écriture se logue en
//! `error!` et n'échoue jamais la mutation appelante.
//!
//! **Origine best-effort :** [`Actor`] lit un en-tête `X-PDO-Actor` déclaratif
//! (`ui` / `cli` / `unknown`) stocké en colonne `actor_hint` — jamais `actor`.
//! Le daemon bind 0.0.0.0 sans auth : l'origine est un indice falsifiable,
//! jamais un gate de comportement.

use serde::Serialize;
use sqlx::{Row, SqlitePool};

/// Une ligne d'audit relue pour le feed `GET /audit` (décroissant, newest-first).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AuditEntry {
    pub id: i64,
    pub ts: String,
    pub actor_hint: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<serde_json::Value>,
}

/// Une écriture d'audit en attente, remise à [`record`] / [`record_best_effort`].
/// Deux colonnes explicites `before`/`after` (pas un `detail` fourre-tout) : le
/// feed doit rendre le *avant → après*.
#[derive(Debug, Clone)]
pub(crate) struct NewAuditEntry {
    pub actor_hint: String,
    pub action: String,
    pub target_kind: Option<String>,
    pub target_id: Option<String>,
    pub before: Option<serde_json::Value>,
    pub after: Option<serde_json::Value>,
}

/// Crée la table (et son index temporel) si absente. `CREATE TABLE IF NOT
/// EXISTS`, aucune `ALTER`, aucun backfill → zéro risque sur un `pdo.db` prod.
pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS audit_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            ts          TEXT NOT NULL,
            actor_hint  TEXT NOT NULL,
            action      TEXT NOT NULL,
            target_kind TEXT,
            target_id   TEXT,
            before      TEXT,
            after       TEXT
        )",
    )
    .execute(db)
    .await?;
    sqlx::query("CREATE INDEX IF NOT EXISTS idx_audit_log_ts ON audit_log(ts)")
        .execute(db)
        .await?;
    Ok(())
}

/// Écrit une entrée. `before`/`after` sont sérialisés en TEXT JSON (SQLite n'a
/// pas de type JSON réel — comme `Event.payload`).
pub(crate) async fn record(db: &SqlitePool, e: NewAuditEntry) -> Result<(), sqlx::Error> {
    let before = e.before.as_ref().map(|v| v.to_string());
    let after = e.after.as_ref().map(|v| v.to_string());
    sqlx::query(
        "INSERT INTO audit_log (ts, actor_hint, action, target_kind, target_id, before, after)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(crate::event_log::now_iso())
    .bind(&e.actor_hint)
    .bind(&e.action)
    .bind(&e.target_kind)
    .bind(&e.target_id)
    .bind(before)
    .bind(after)
    .execute(db)
    .await?;
    Ok(())
}

/// Best-effort (ADR-0044) : ne fait **jamais** échouer la mutation appelante ;
/// un échec d'écriture se logue en `error!` (jamais `warn!` — une entrée d'audit
/// manquante est un vrai trou d'observabilité, pas un avertissement mineur).
pub(crate) async fn record_best_effort(db: &SqlitePool, e: NewAuditEntry) {
    let (action, target) = (e.action.clone(), e.target_id.clone());
    if let Err(err) = record(db, e).await {
        tracing::error!(action = %action, target_id = ?target, "audit_log: écriture échouée: {err}");
    }
}

/// Lecture filtrée, newest-first. `id DESC` seul suffit (`INTEGER PRIMARY KEY
/// AUTOINCREMENT` est déjà un ordre total — pas de tiebreak sur `ts`). Les
/// quatre filtres sont optionnels : par défaut un feed global décroissant, où
/// l'on découvre *quelle* cible dans le flux. `[from, to)` : borne basse
/// incluse, borne haute exclue.
pub(crate) async fn list(
    db: &SqlitePool,
    from: Option<&str>,
    to: Option<&str>,
    target_kind: Option<&str>,
    target_id: Option<&str>,
    limit: i64,
) -> Result<Vec<AuditEntry>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, ts, actor_hint, action, target_kind, target_id, before, after
         FROM audit_log
         WHERE (?1 IS NULL OR ts >= ?1)
           AND (?2 IS NULL OR ts < ?2)
           AND (?3 IS NULL OR target_kind = ?3)
           AND (?4 IS NULL OR target_id = ?4)
         ORDER BY id DESC
         LIMIT ?5",
    )
    .bind(from)
    .bind(to)
    .bind(target_kind)
    .bind(target_id)
    .bind(limit)
    .fetch_all(db)
    .await?;
    rows.iter().map(row_to_entry).collect()
}

/// Un TEXT `before`/`after` malformé (jamais écrit par [`record`], mais on ne
/// panique pas dessus) dégrade en `None`.
fn row_to_entry(row: &sqlx::sqlite::SqliteRow) -> Result<AuditEntry, sqlx::Error> {
    let before: Option<String> = row.try_get("before")?;
    let after: Option<String> = row.try_get("after")?;
    Ok(AuditEntry {
        id: row.get("id"),
        ts: row.get("ts"),
        actor_hint: row.get("actor_hint"),
        action: row.get("action"),
        target_kind: row.try_get("target_kind")?,
        target_id: row.try_get("target_id")?,
        before: before.and_then(|s| serde_json::from_str(&s).ok()),
        after: after.and_then(|s| serde_json::from_str(&s).ok()),
    })
}

/// Origine déclarative d'une mutation de config, lue de l'en-tête `X-PDO-Actor`.
/// **Falsifiable** (bind 0.0.0.0, aucune auth) : c'est un indice de traçabilité
/// stocké en `actor_hint`, jamais un gate de comportement. `Cli` est reconnu par
/// forward-proofing bien qu'aucune sous-commande CLI ne mute de Trigger en v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Actor {
    Ui,
    Cli,
    Unknown,
}

impl Actor {
    pub(crate) fn as_hint(self) -> &'static str {
        match self {
            Actor::Ui => "ui",
            Actor::Cli => "cli",
            Actor::Unknown => "unknown",
        }
    }
}

/// Le premier extracteur `FromRequestParts` du crate. Ne rejette **jamais**
/// (`Infallible`) : un en-tête absent ou inconnu vaut [`Actor::Unknown`], pas un
/// 4xx — « origine inconnue » est acceptable, « aucune entrée » ne l'est pas.
/// À placer avant le consommateur de corps (`Json<T>`) dans les signatures de
/// handler (axum exige le body-extractor en dernier).
impl<S> axum::extract::FromRequestParts<S> for Actor
where
    S: Send + Sync,
{
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        Ok(
            match parts
                .headers
                .get("X-PDO-Actor")
                .and_then(|v| v.to_str().ok())
            {
                Some("ui") => Actor::Ui,
                Some("cli") => Actor::Cli,
                _ => Actor::Unknown,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::extract::FromRequestParts;

    async fn test_db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init(&db).await.unwrap();
        db
    }

    fn trigger_entry(action: &str, id: &str) -> NewAuditEntry {
        NewAuditEntry {
            actor_hint: "ui".to_string(),
            action: action.to_string(),
            target_kind: Some("trigger".to_string()),
            target_id: Some(id.to_string()),
            before: None,
            after: None,
        }
    }

    #[tokio::test]
    async fn record_then_list_roundtrips_before_and_after_json() {
        // A patch-shaped entry carries both snapshots; `list` must give them back
        // byte-for-byte as JSON, not as re-escaped strings.
        let db = test_db().await;
        record(
            &db,
            NewAuditEntry {
                actor_hint: "ui".to_string(),
                action: "trigger.updated".to_string(),
                target_kind: Some("trigger".to_string()),
                target_id: Some("t-1".to_string()),
                before: Some(serde_json::json!({"enabled": true, "name": "nightly"})),
                after: Some(serde_json::json!({"enabled": false, "name": "nightly"})),
            },
        )
        .await
        .unwrap();

        let rows = list(&db, None, None, None, None, 200).await.unwrap();
        assert_eq!(rows.len(), 1);
        let e = &rows[0];
        assert_eq!(e.actor_hint, "ui");
        assert_eq!(e.action, "trigger.updated");
        assert_eq!(e.target_kind.as_deref(), Some("trigger"));
        assert_eq!(e.target_id.as_deref(), Some("t-1"));
        assert_eq!(
            e.before.as_ref().unwrap()["enabled"],
            serde_json::json!(true)
        );
        assert_eq!(
            e.after.as_ref().unwrap()["enabled"],
            serde_json::json!(false)
        );
        assert!(!e.ts.is_empty(), "ts is stamped by now_iso()");
    }

    #[tokio::test]
    async fn create_and_delete_null_snapshots_survive() {
        // create → before NULL, after set; delete → before set, after NULL. Both
        // NULLs must round-trip as `None`, not as a JSON `null` Value.
        let db = test_db().await;
        record(
            &db,
            NewAuditEntry {
                after: Some(serde_json::json!({"id": "t-1"})),
                ..trigger_entry("trigger.created", "t-1")
            },
        )
        .await
        .unwrap();
        record(
            &db,
            NewAuditEntry {
                before: Some(serde_json::json!({"id": "t-1"})),
                ..trigger_entry("trigger.deleted", "t-1")
            },
        )
        .await
        .unwrap();

        let rows = list(&db, None, None, None, None, 200).await.unwrap();
        // newest-first: delete then create.
        assert_eq!(rows[0].action, "trigger.deleted");
        assert!(rows[0].before.is_some());
        assert!(rows[0].after.is_none());
        assert_eq!(rows[1].action, "trigger.created");
        assert!(rows[1].before.is_none());
        assert!(rows[1].after.is_some());
    }

    #[tokio::test]
    async fn list_is_newest_first_by_id() {
        let db = test_db().await;
        for i in 0..5 {
            record(&db, trigger_entry("trigger.updated", &format!("t-{i}")))
                .await
                .unwrap();
        }
        let rows = list(&db, None, None, None, None, 200).await.unwrap();
        let ids: Vec<i64> = rows.iter().map(|e| e.id).collect();
        let mut sorted = ids.clone();
        sorted.sort_unstable();
        sorted.reverse();
        assert_eq!(ids, sorted, "id DESC is a total order, newest first");
    }

    #[tokio::test]
    async fn limit_caps_the_read() {
        let db = test_db().await;
        for i in 0..10 {
            record(&db, trigger_entry("trigger.updated", &format!("t-{i}")))
                .await
                .unwrap();
        }
        let rows = list(&db, None, None, None, None, 3).await.unwrap();
        assert_eq!(rows.len(), 3, "LIMIT bounds the feed");
    }

    #[tokio::test]
    async fn target_filters_narrow_to_one_trigger() {
        let db = test_db().await;
        record(&db, trigger_entry("trigger.updated", "t-1"))
            .await
            .unwrap();
        record(&db, trigger_entry("trigger.updated", "t-2"))
            .await
            .unwrap();
        record(
            &db,
            NewAuditEntry {
                target_kind: Some("instance".to_string()),
                target_id: None,
                ..trigger_entry("triggers.pause_changed", "ignored")
            },
        )
        .await
        .unwrap();

        let only_t1 = list(&db, None, None, Some("trigger"), Some("t-1"), 200)
            .await
            .unwrap();
        assert_eq!(only_t1.len(), 1);
        assert_eq!(only_t1[0].target_id.as_deref(), Some("t-1"));

        let only_instance = list(&db, None, None, Some("instance"), None, 200)
            .await
            .unwrap();
        assert_eq!(only_instance.len(), 1);
        assert_eq!(only_instance[0].action, "triggers.pause_changed");
    }

    #[tokio::test]
    async fn from_to_window_is_half_open() {
        // `[from, to)`: `from` inclusive, `to` exclusive. Bind the rows' own
        // timestamps as bounds to assert the boundary semantics precisely.
        let db = test_db().await;
        for i in 0..3 {
            record(&db, trigger_entry("trigger.updated", &format!("t-{i}")))
                .await
                .unwrap();
        }
        let all = list(&db, None, None, None, None, 200).await.unwrap();
        assert_eq!(all.len(), 3);
        // ascending by ts to pick boundaries
        let mut ts: Vec<String> = all.iter().map(|e| e.ts.clone()).collect();
        ts.sort();

        // from = middle ts (inclusive) → drops nothing strictly older.
        let from_mid = list(&db, Some(ts[1].as_str()), None, None, None, 200)
            .await
            .unwrap();
        assert!(
            from_mid.iter().all(|e| e.ts >= ts[1]),
            "from is an inclusive lower bound"
        );

        // to = max ts (exclusive) → the newest row is excluded.
        let to_max = list(&db, None, Some(ts[2].as_str()), None, None, 200)
            .await
            .unwrap();
        assert!(
            to_max.iter().all(|e| e.ts < ts[2]),
            "to is an exclusive upper bound"
        );
    }

    #[tokio::test]
    async fn record_best_effort_swallows_a_write_error() {
        // A failed audit write must NEVER propagate: on a closed pool the
        // best-effort record logs and returns, never panics or blocks.
        let db = test_db().await;
        db.close().await;
        record_best_effort(&db, trigger_entry("trigger.updated", "t-1")).await;
    }

    async fn actor_from_header(value: Option<&str>) -> Actor {
        let mut builder = axum::http::Request::builder();
        if let Some(v) = value {
            builder = builder.header("X-PDO-Actor", v);
        }
        let req = builder.body(axum::body::Body::empty()).unwrap();
        let (mut parts, _) = req.into_parts();
        Actor::from_request_parts(&mut parts, &())
            .await
            .expect("Actor extraction is Infallible")
    }

    #[tokio::test]
    async fn actor_extractor_maps_header_and_never_rejects() {
        assert_eq!(actor_from_header(Some("ui")).await, Actor::Ui);
        assert_eq!(actor_from_header(Some("cli")).await, Actor::Cli);
        assert_eq!(actor_from_header(Some("bogus")).await, Actor::Unknown);
        assert_eq!(actor_from_header(None).await, Actor::Unknown);
        // as_hint is the byte-exact column value.
        assert_eq!(Actor::Ui.as_hint(), "ui");
        assert_eq!(Actor::Cli.as_hint(), "cli");
        assert_eq!(Actor::Unknown.as_hint(), "unknown");
    }
}
