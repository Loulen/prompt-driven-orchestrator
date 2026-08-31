//! Instance-wide configuration store (#129, ADR-0015).
//!
//! Three daemon-wide runtime knobs — the concurrent-session cap, the tmux reaper
//! TTL, and the Trigger guard timeout — historically lived only in environment
//! variables (shell + restart). This module is their durable home: a SQLite
//! *singleton* row (`id = 1`) in `pdo.db`, mirroring [`crate::trigger_store`]
//! (config + mutable state, not a canvas-backed artifact → wrong fit for YAML,
//! cf. CONTEXT.md *Persistence — table SQLite*).
//!
//! **Load-bearing:** every column is NULLABLE and the seed row is all-NULL. A
//! NULL stored value means "unset" and the resolver falls through to the env
//! var, then the code default — the `stored → env → default` precedence of
//! ADR-0015. Keeping the columns NULLABLE (rather than `NOT NULL DEFAULT 20`) is
//! deliberate: a non-null default would make the stored tier always win, so the
//! env would never be consulted and the existing cap/TTL tests (which set env,
//! expect env) would break.
//!
//! The public API mirrors the store idiom: `init` (create + seed), `get`
//! (singleton fetch), and `update` (partial, set-only edit). Precedence itself
//! lives with each reader (`admission::configured_cap_with`,
//! `tmux_session_manager::reaper_ttl_with`, `guard_runner::guard_timeout_with`);
//! this module only persists the `stored` tier.

use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};

/// The persisted singleton config row. Each field is `None` when unset (the
/// resolver then falls through to env → default).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct InstanceConfig {
    /// Stored global session cap, or `None` when unset.
    pub session_cap: Option<i64>,
    /// Stored tmux reaper TTL in seconds, or `None` when unset.
    pub reaper_ttl_secs: Option<i64>,
    /// Stored idle TTL of the shared library assistant in seconds, or `None` when
    /// unset (#594, ADR-0051). Its own knob, not a reuse of
    /// [`Self::reaper_ttl_secs`]: that one bounds a *completed node's* session
    /// (default 1 h, an operator may want to read it later), this one bounds an
    /// assistant nobody is attached to and nobody is editing against (default
    /// 120 s). The resolver
    /// ([`crate::tmux_session_manager::libassist_idle_ttl_with`]) owns the
    /// precedence.
    pub libassist_idle_ttl_secs: Option<i64>,
    /// Stored Trigger guard timeout in **seconds**, or `None` when unset. Note
    /// the env seam ([`crate::guard_runner::GUARD_TIMEOUT_MS_OVERRIDE_ENV`]) is
    /// in milliseconds; the stored value is seconds (ADR-0015).
    pub guard_timeout_secs: Option<i64>,
    /// Stored instance-wide default Claude model, or `None` when unset (#347).
    /// A work node with no `model:` override launches with this model; `None`
    /// falls through to the env seam ([`crate::tmux_session_manager::DEFAULT_MODEL_ENV`])
    /// then the account default (no `--model`). Free-text pass-through, no enum
    /// (ADR-0001): an invalid id fails loud in `claude`.
    pub default_model: Option<String>,
    /// Stored instance-wide default sandbox mode (`"off"` | `"full"` | `"minimal"`), or
    /// `None` when unset (#410). `None` falls through to the env seam
    /// ([`crate::event_log::DEFAULT_SANDBOX_ENV`]) then the built-in default (`off`,
    /// the byte-identical host path). `Some("")` is the clear sentinel — [`update`]
    /// normalises it to SQL `NULL` so it never wins precedence. The resolver
    /// ([`crate::event_log::default_sandbox_with`]) owns the precedence; the create-run
    /// chokepoint then feeds it through [`crate::event_log::effective_sandbox`].
    pub default_sandbox: Option<String>,
    /// Stored turn-end auto-completion flag as `0`/`1`, or `None` when unset (#469).
    /// `None` falls through to the env seam
    /// ([`crate::stale_detector::AUTOCOMPLETE_TURN_END_ENV`]) then the built-in
    /// default (`false`). The resolver
    /// ([`crate::stale_detector::autocomplete_turn_end_with`]) owns the precedence.
    ///
    /// `Option<i64>` and not `Option<bool>`: every other column on this table is a
    /// nullable scalar and `NULL` is what makes the `stored → env → default`
    /// fall-through work. A `bool` column would have to choose between `NOT NULL
    /// DEFAULT 0` (the stored tier then always wins, shadowing the env) and being
    /// nullable anyway.
    pub autocomplete_turn_end: Option<i64>,
    /// Stored instance-wide default for auto-naming a Run as `0`/`1`, or `None` when
    /// unset (#338). `None` falls through to the env seam
    /// ([`crate::prompt_augmenter::DEFAULT_AUTO_NAME_ENV`]) then the built-in default
    /// (`true`, the pre-#338 behaviour). The resolver
    /// ([`crate::prompt_augmenter::default_auto_name_with`]) owns the precedence; the
    /// create-run chokepoint reads it FRESH at the edge (never cached at boot) and only
    /// when the request neither supplies an explicit `auto_name` flag nor a `name`.
    ///
    /// `Option<i64>` and not `Option<bool>` for the same reason as
    /// [`Self::autocomplete_turn_end`]: `NULL` is what makes the `stored → env → default`
    /// fall-through work; a stored `0` is a decision that beats the env.
    pub default_auto_name: Option<i64>,
    /// Stored instance-wide default **harness** (`claude`, `opencode`), or `None`
    /// when unset (#550, ADR-0046 amending ADR-0015). `None` falls through to the
    /// env seam ([`crate::tmux_session_manager::DEFAULT_HARNESS_ENV`]) then the
    /// `claude` floor. Free-text pass-through, no enum (ADR-0001). `Some("")` is
    /// the clear sentinel — [`update`] normalises it to SQL `NULL`.
    pub default_harness: Option<String>,
    /// Stored instance-wide default **model per harness** (#550, ADR-0046): a map
    /// `harness → model`, e.g. `{"claude":"opus"}`. The single `default_model`
    /// became per-harness because a model slug means nothing outside its harness.
    /// Empty map ⇒ no per-harness default (for `claude` the legacy
    /// [`crate::tmux_session_manager::DEFAULT_MODEL_ENV`] still contributes at the
    /// spawn seam). Persisted as a JSON object in the `default_harness_model`
    /// column (`trigger_store::variables` idiom).
    #[serde(default)]
    pub default_harness_model: std::collections::BTreeMap<String, String>,
    /// Stored instance-wide `auto_fail` default as `0`/`1`, or `None` when unset
    /// (ADR-0049). The **global** (coarsest) tier of the `node → Run → Projet →
    /// instance` precedence resolved by [`crate::auto_fail::resolve_auto_fail`].
    /// Checked, an agent's `pdo fail` terminalises the run directly to `Failed`;
    /// unset/`0`, it parks the run `AwaitingUser` for a human to confirm. `None`
    /// falls through to the env seam ([`AUTO_FAIL_ENV`]) then the built-in default
    /// (`false`).
    ///
    /// `Option<i64>` and not `Option<bool>` for the same reason as
    /// [`Self::autocomplete_turn_end`]: `NULL` makes the `stored → env → default`
    /// fall-through work; a stored `0` is a decision that beats the env.
    pub auto_fail: Option<i64>,
    /// The Instance tier of the agentic-profile union (#563, ADR-0057) — the
    /// **coarsest** tier `agent_choice::resolve` consults, just above the
    /// reserved Default profile floor. `None` (unset) preserves the pre-#563
    /// legacy precedence exactly: [`Self::default_harness`] /
    /// [`Self::default_harness_model`] still apply. `Some(Profile | Custom)`
    /// wins outright over them at this tier (never merges). Persisted as a
    /// JSON object in a nullable TEXT column, same idiom as
    /// [`Self::default_harness_model`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_choice: Option<crate::agent_choice::AgentChoice>,
    /// RFC3339-millis UTC timestamp of the last write (or the seed).
    pub updated_at: String,
}

/// A partial edit. A `None` field leaves the stored value untouched.
///
/// The three numeric knobs are a set-only MVP: there is no path to clear a
/// stored value back to unset (a richer double-`Option` form was considered and
/// deferred — ADR-0015). `default_model` is the first *clear-capable* column
/// (#347): it stays a plain `Option<String>`, but `Some("")` is the clear
/// sentinel — [`update`] normalises an empty string to SQL `NULL` (a `Some("")`
/// must never persist, or it would win precedence and shadow env/account
/// default). This keeps the double-`Option` still deferred while giving the UI
/// a "Default" (unset) affordance.
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct UpdateInstanceConfig {
    pub session_cap: Option<i64>,
    pub reaper_ttl_secs: Option<i64>,
    /// Set the shared library assistant's idle TTL in seconds (#594). Set-only,
    /// like the other numeric knobs.
    pub libassist_idle_ttl_secs: Option<i64>,
    pub guard_timeout_secs: Option<i64>,
    pub default_model: Option<String>,
    /// Set the instance-wide default sandbox mode (#410). `Some("")` clears it back to
    /// unset (same `""`-sentinel as `default_model`); `None` leaves it untouched.
    pub default_sandbox: Option<String>,
    /// Set turn-end auto-completion (#469): `Some(true)` stores `1`, `Some(false)`
    /// stores `0`, `None` leaves it untouched.
    ///
    /// A `bool` on the wire (the UI has a checkbox, not a number) but a set-only
    /// knob like the numeric ones: there is no path back to *unset*, so a user who
    /// wants the env tier to decide again clears the column out of band. Storing
    /// `0` rather than `NULL` for "off" is deliberate — unticking the box must
    /// override a `PDO_AUTOCOMPLETE_TURN_END=1`, which a `NULL` would not.
    pub autocomplete_turn_end: Option<bool>,
    /// Set the instance-wide default for auto-naming Runs (#338): `Some(true)` stores
    /// `1`, `Some(false)` stores `0`, `None` leaves it untouched.
    ///
    /// Same set-only, `0`-not-`NULL` discipline as [`Self::autocomplete_turn_end`]:
    /// storing `0` for "off" is what lets unticking the box override a
    /// `PDO_DEFAULT_AUTO_NAME=1`, which a `NULL` fall-through would not.
    pub default_auto_name: Option<bool>,
    /// Set the instance-wide default harness (#550). `Some("")` clears it back to
    /// unset (same `""`-sentinel as `default_model`); `None` leaves it untouched.
    pub default_harness: Option<String>,
    /// Set the instance-wide per-harness default model map (#550). `Some(map)`
    /// replaces the stored map wholesale; an empty map clears it. `None` leaves it
    /// untouched.
    pub default_harness_model: Option<std::collections::BTreeMap<String, String>>,
    /// Set the instance-wide `auto_fail` default (ADR-0049): `Some(true)` stores
    /// `1`, `Some(false)` stores `0`, `None` leaves it untouched. Same set-only,
    /// `0`-not-`NULL` discipline as [`Self::autocomplete_turn_end`].
    pub auto_fail: Option<bool>,
    /// Set the Instance tier of the agentic-profile union (#563): `Some(None)`
    /// clears it back to unset (legacy `default_harness`/`default_harness_model`
    /// decide again); `Some(Some(choice))` sets it; `None` leaves it untouched.
    /// Double-`Option`, mirroring [`crate::trigger_store::UpdateTrigger::harness`]
    /// — this column, unlike the set-only numeric knobs, needs a genuine "clear"
    /// path back to legacy precedence.
    pub agent_choice: Option<Option<crate::agent_choice::AgentChoice>>,
}

impl UpdateInstanceConfig {
    fn is_empty(&self) -> bool {
        self.session_cap.is_none()
            && self.reaper_ttl_secs.is_none()
            && self.libassist_idle_ttl_secs.is_none()
            && self.guard_timeout_secs.is_none()
            && self.default_model.is_none()
            && self.default_sandbox.is_none()
            && self.autocomplete_turn_end.is_none()
            && self.default_auto_name.is_none()
            && self.default_harness.is_none()
            && self.default_harness_model.is_none()
            && self.auto_fail.is_none()
            && self.agent_choice.is_none()
    }
}

/// Create the `instance_config` table if absent and seed the all-NULL singleton
/// row. Idempotent: safe to call on every boot.
///
/// Future knobs are added via an idempotent PRAGMA-guarded
/// `ALTER TABLE … ADD COLUMN` (precedent: `max_concurrent` #239 in
/// `trigger_store`), never a migration runner.
pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS instance_config (
            id                 INTEGER PRIMARY KEY CHECK (id = 1),
            session_cap        INTEGER,
            reaper_ttl_secs    INTEGER,
            guard_timeout_secs INTEGER,
            default_model      TEXT,
            triggers_paused    INTEGER,
            default_sandbox    TEXT,
            autocomplete_turn_end INTEGER,
            default_auto_name  INTEGER,
            default_harness    TEXT,
            default_harness_model TEXT,
            auto_fail          INTEGER,
            libassist_idle_ttl_secs INTEGER,
            agent_choice       TEXT,
            updated_at         TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;

    // Seed the singleton row with all-NULL knobs. `INSERT OR IGNORE` is a no-op
    // once the row exists, so this cannot clobber a stored config on a restart.
    let now = crate::event_log::now_iso();
    sqlx::query("INSERT OR IGNORE INTO instance_config (id, updated_at) VALUES (1, ?)")
        .bind(&now)
        .execute(db)
        .await?;

    // Additive migration for pre-#347 databases: the `default_model` column is
    // absent on tables created before it joined the `CREATE TABLE` above. Add it
    // idempotently, PRAGMA-guarded (the first `ALTER` this module has needed;
    // precedent: `trigger_store` `max_concurrent`, #239). Never a migration
    // runner — a single guarded `ADD COLUMN`, safe on every boot.
    let has_default_model = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'default_model'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_default_model {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN default_model TEXT")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#348: the global trigger kill-switch.
    let has_triggers_paused = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'triggers_paused'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_triggers_paused {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN triggers_paused INTEGER")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#410. NULLABLE, never `NOT NULL DEFAULT`: a non-null
    // default would make the stored tier always win and break the `off`-by-default
    // (byte-identical) precedence floor. Same for every column below.
    let has_default_sandbox = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'default_sandbox'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_default_sandbox {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN default_sandbox TEXT")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#469: NULLABLE keeps an existing install OFF (ADR-0012)
    // with the env tier still able to speak.
    let has_autocomplete_turn_end = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'autocomplete_turn_end'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_autocomplete_turn_end {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN autocomplete_turn_end INTEGER")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#338: NULLABLE preserves the pre-#338 behaviour (a
    // nameless Run is auto-named by the manager).
    let has_default_auto_name = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'default_auto_name'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_default_auto_name {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN default_auto_name INTEGER")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#550: the multi-harness axis.
    let has_default_harness = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'default_harness'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_default_harness {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN default_harness TEXT")
            .execute(db)
            .await?;
    }
    let has_default_harness_model = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'default_harness_model'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_default_harness_model {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN default_harness_model TEXT")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-ADR-0049: `auto_fail`.
    let has_auto_fail =
        sqlx::query("SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'auto_fail'")
            .fetch_optional(db)
            .await?
            .is_some();
    if !has_auto_fail {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN auto_fail INTEGER")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#594: the shared, idle-reaped library assistant.
    let has_libassist_idle_ttl = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'libassist_idle_ttl_secs'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_libassist_idle_ttl {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN libassist_idle_ttl_secs INTEGER")
            .execute(db)
            .await?;
    }

    // Additive migration, pre-#563: NULLABLE so an existing install falls through
    // to the legacy `default_harness`/`default_harness_model` precedence unchanged.
    let has_agent_choice = sqlx::query(
        "SELECT 1 FROM pragma_table_info('instance_config') WHERE name = 'agent_choice'",
    )
    .fetch_optional(db)
    .await?
    .is_some();
    if !has_agent_choice {
        sqlx::query("ALTER TABLE instance_config ADD COLUMN agent_choice TEXT")
            .execute(db)
            .await?;
    }

    Ok(())
}

fn row_to_config(row: &sqlx::sqlite::SqliteRow) -> InstanceConfig {
    InstanceConfig {
        session_cap: row.get("session_cap"),
        reaper_ttl_secs: row.get("reaper_ttl_secs"),
        libassist_idle_ttl_secs: row.get("libassist_idle_ttl_secs"),
        guard_timeout_secs: row.get("guard_timeout_secs"),
        default_model: row.get("default_model"),
        default_sandbox: row.get("default_sandbox"),
        autocomplete_turn_end: row.get("autocomplete_turn_end"),
        default_auto_name: row.get("default_auto_name"),
        default_harness: row.get("default_harness"),
        // Stored as a JSON object in a TEXT column (trigger_store::variables
        // idiom). NULL / empty / unparseable ⇒ an empty map, so a fresh row and a
        // corrupt one both fall through to env → floor rather than erroring.
        default_harness_model: row
            .get::<Option<String>, _>("default_harness_model")
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default(),
        auto_fail: row.get("auto_fail"),
        // #563: NULL / unparseable ⇒ `None` — the tier is simply transparent,
        // never an error (mirrors `default_harness_model`'s degrade-to-empty).
        agent_choice: row
            .get::<Option<String>, _>("agent_choice")
            .and_then(|s| serde_json::from_str(&s).ok()),
        updated_at: row.get("updated_at"),
    }
}

/// Fetch the singleton config row. Always present after [`init`].
pub(crate) async fn get(db: &SqlitePool) -> Result<InstanceConfig, sqlx::Error> {
    let row = sqlx::query("SELECT * FROM instance_config WHERE id = 1")
        .fetch_optional(db)
        .await?;
    row.as_ref()
        .map(row_to_config)
        .ok_or(sqlx::Error::RowNotFound)
}

/// Apply a partial config edit and return the updated row. A no-op edit (no
/// field set) returns the current row unchanged and does not bump `updated_at`.
pub(crate) async fn update(
    db: &SqlitePool,
    edit: UpdateInstanceConfig,
) -> Result<InstanceConfig, sqlx::Error> {
    if edit.is_empty() {
        return get(db).await;
    }

    // Build the SET clause field-by-field, then bind in the same order so the
    // positional placeholders line up (mirrors trigger_store::update).
    let mut sets: Vec<&str> = Vec::new();
    if edit.session_cap.is_some() {
        sets.push("session_cap = ?");
    }
    if edit.reaper_ttl_secs.is_some() {
        sets.push("reaper_ttl_secs = ?");
    }
    if edit.libassist_idle_ttl_secs.is_some() {
        sets.push("libassist_idle_ttl_secs = ?");
    }
    if edit.guard_timeout_secs.is_some() {
        sets.push("guard_timeout_secs = ?");
    }
    if edit.default_model.is_some() {
        sets.push("default_model = ?");
    }
    if edit.default_sandbox.is_some() {
        sets.push("default_sandbox = ?");
    }
    if edit.autocomplete_turn_end.is_some() {
        sets.push("autocomplete_turn_end = ?");
    }
    if edit.default_auto_name.is_some() {
        sets.push("default_auto_name = ?");
    }
    if edit.default_harness.is_some() {
        sets.push("default_harness = ?");
    }
    if edit.default_harness_model.is_some() {
        sets.push("default_harness_model = ?");
    }
    if edit.auto_fail.is_some() {
        sets.push("auto_fail = ?");
    }
    if edit.agent_choice.is_some() {
        sets.push("agent_choice = ?");
    }
    // Always bump the write timestamp on a real edit.
    sets.push("updated_at = ?");

    let sql = format!(
        "UPDATE instance_config SET {} WHERE id = 1",
        sets.join(", ")
    );
    let mut query = sqlx::query(&sql);
    if let Some(v) = edit.session_cap {
        query = query.bind(v);
    }
    if let Some(v) = edit.reaper_ttl_secs {
        query = query.bind(v);
    }
    if let Some(v) = edit.libassist_idle_ttl_secs {
        query = query.bind(v);
    }
    if let Some(v) = edit.guard_timeout_secs {
        query = query.bind(v);
    }
    if let Some(v) = edit.default_model {
        // "" is the clear sentinel → bind SQL NULL (#347). A `Some("")` must
        // never persist: it would win the `stored → env → default` precedence
        // and both shadow the env/account default and reach the tail as an
        // empty `--model` value. Storing NULL restores fall-through.
        query = query.bind(if v.is_empty() { None } else { Some(v) });
    }
    if let Some(v) = edit.default_sandbox {
        // "" = clear sentinel → SQL NULL (#410, mirrors default_model). The resolver
        // then falls back env→default(off, the byte-identical host path).
        query = query.bind(if v.is_empty() { None } else { Some(v) });
    }
    if let Some(v) = edit.autocomplete_turn_end {
        // 0/1, never NULL: "off" must be a stored decision that beats the env
        // tier, not a fall-through (#469).
        query = query.bind(if v { 1_i64 } else { 0_i64 });
    }
    if let Some(v) = edit.default_auto_name {
        // 0/1, never NULL: unticking must persist a stored `0` that beats a
        // `PDO_DEFAULT_AUTO_NAME=1`, not fall through to it (#338).
        query = query.bind(if v { 1_i64 } else { 0_i64 });
    }
    if let Some(v) = edit.default_harness {
        // "" = clear sentinel → SQL NULL (#550, mirrors default_model): a stored
        // "" would win precedence and shadow env/floor. NULL restores fall-through.
        query = query.bind(if v.is_empty() { None } else { Some(v) });
    }
    if let Some(v) = edit.default_harness_model {
        // Persist the map as JSON; an empty map stores `{}` (a real "no defaults"
        // decision). Serialisation cannot fail for a String→String map.
        query = query.bind(serde_json::to_string(&v).unwrap_or_else(|_| "{}".to_string()));
    }
    if let Some(v) = edit.auto_fail {
        // 0/1, never NULL: unchecking must persist a stored `0` that beats a
        // `PDO_AUTO_FAIL=1`, not fall through to it (ADR-0049).
        query = query.bind(if v { 1_i64 } else { 0_i64 });
    }
    if let Some(v) = edit.agent_choice {
        // #563: `Some(None)` → SQL NULL (fall back to legacy `default_harness`/
        // `default_harness_model`); `Some(Some(choice))` → the serialized JSON.
        query = query.bind(v.map(|c| serde_json::to_string(&c).unwrap_or_default()));
    }
    query = query.bind(crate::event_log::now_iso());
    query.execute(db).await?;

    get(db).await
}

/// Read the global Trigger pause flag (#348). `NULL`/`0` ≡ not paused, `1` ≡
/// paused.
///
/// This flag is a **daemon-wide scheduler gate**, deliberately kept OUTSIDE the
/// `stored → env → default` `/settings` machinery ([`InstanceConfig`],
/// [`UpdateInstanceConfig`], `build_settings_view`, `put_settings`): a boolean
/// kill-switch has no env tier and no meaningful default other than "off", so
/// the per-field settings view would only add noise. It rides the same
/// singleton `instance_config` row purely for storage — the first knob on that
/// table excluded from the settings resolver.
pub(crate) async fn triggers_paused(db: &SqlitePool) -> Result<bool, sqlx::Error> {
    let v: Option<i64> =
        sqlx::query_scalar("SELECT triggers_paused FROM instance_config WHERE id = 1")
            .fetch_optional(db)
            .await?
            .flatten();
    Ok(v == Some(1))
}

/// Set (or clear) the global Trigger pause flag (#348). Unpausing stores SQL
/// `NULL` (≡ not paused) rather than `0`, keeping the "unset ≡ off" convention
/// the rest of the table follows. Bumps `updated_at` like [`update`].
pub(crate) async fn set_triggers_paused(db: &SqlitePool, paused: bool) -> Result<(), sqlx::Error> {
    sqlx::query("UPDATE instance_config SET triggers_paused = ?, updated_at = ? WHERE id = 1")
        .bind(if paused { Some(1_i64) } else { None })
        .bind(crate::event_log::now_iso())
        .execute(db)
        .await?;
    Ok(())
}

/// Env seam for the instance-wide `auto_fail` default (ADR-0049 / ADR-0015).
/// Read only inside [`resolve_instance_auto_fail`], the `stored → env → default`
/// precedence — never elsewhere. Any of `1`/`true`/`yes`/`on` (case-insensitive)
/// is truthy; anything else (incl. unset) is falsey.
pub(crate) const AUTO_FAIL_ENV: &str = "PDO_AUTO_FAIL";

/// The instance (global) tier of `auto_fail`, `stored → env → default(false)`
/// (ADR-0049). `stored` is the `auto_fail` column value (`Some(0/1)` a stored
/// decision, `None` unset). Pure over its `stored` arg; reads the env only when
/// `stored` is `None`, so a stored `0` beats a `PDO_AUTO_FAIL=1`.
pub(crate) fn resolve_instance_auto_fail(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => std::env::var(AUTO_FAIL_ENV)
            .ok()
            .map(|s| {
                let t = s.trim();
                t.eq_ignore_ascii_case("1")
                    || t.eq_ignore_ascii_case("true")
                    || t.eq_ignore_ascii_case("yes")
                    || t.eq_ignore_ascii_case("on")
            })
            .unwrap_or(false),
    }
}

/// The two columns #471 retired: `image_source` (#411) and `dockerfile_path` (#431). A staging
/// profile's own image source replaced both, and a value left in either column is now **inert**.
///
/// Ordered as the boot warning should read them, and paired with the env var that survived — the
/// only remaining instance-wide way to move that same knob.
pub(crate) const RETIRED_SANDBOX_IMAGE_COLUMNS: &[(&str, &str)] = &[
    ("image_source", crate::sandbox_image::IMAGE_SOURCE_ENV),
    ("dockerfile_path", crate::sandbox_image::DOCKERFILE_PATH_ENV),
];

/// Read whatever is left in the columns [`RETIRED_SANDBOX_IMAGE_COLUMNS`] names, so the boot can
/// **say so once** instead of letting a value the user set quietly stop mattering (#471 AC5, the
/// same principle as #470).
///
/// Why it is a bespoke reader rather than two more fields on [`InstanceConfig`]: the point is
/// precisely that these are NOT config any more. Keeping them in the struct would put them back on
/// every `get()`, and something would eventually consult one.
///
/// PRAGMA-guarded per column, because the two cases are genuinely different and both are normal:
/// a database created before #471 HAS the columns (`CREATE TABLE IF NOT EXISTS` never dropped
/// them — SQLite `DROP COLUMN` on a live install buys nothing and can fail on old engines), while
/// one created after does NOT. An absent column must be silence, not an error.
///
/// Empty strings are skipped: `""` was the clear sentinel, so it never meant a value.
pub(crate) async fn retired_sandbox_image_values(
    db: &SqlitePool,
) -> Result<Vec<(&'static str, String)>, sqlx::Error> {
    let mut found = Vec::new();
    for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
        let present =
            sqlx::query("SELECT 1 FROM pragma_table_info('instance_config') WHERE name = ?")
                .bind(column)
                .fetch_optional(db)
                .await?
                .is_some();
        if !present {
            continue;
        }
        // The column name cannot be bound as a parameter, and it comes from the constant above —
        // never from a request.
        let value: Option<String> = sqlx::query_scalar(&format!(
            "SELECT {column} FROM instance_config WHERE id = 1"
        ))
        .fetch_optional(db)
        .await?
        .flatten();
        if let Some(v) = value.filter(|v| !v.is_empty()) {
            found.push((*column, v));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn test_db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        init(&db).await.unwrap();
        db
    }

    #[tokio::test]
    async fn fresh_row_is_all_null() {
        // Load-bearing (ADR-0015): a fresh install seeds an all-NULL row so the
        // resolver falls through to env → default. A non-null default here would
        // silently break the env-driven cap/TTL tests.
        let db = test_db().await;
        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.session_cap, None);
        assert_eq!(cfg.reaper_ttl_secs, None);
        assert_eq!(cfg.guard_timeout_secs, None);
        assert_eq!(cfg.default_model, None);
        assert_eq!(cfg.default_sandbox, None);
        assert_eq!(cfg.autocomplete_turn_end, None);
        assert_eq!(cfg.default_auto_name, None);
        assert_eq!(cfg.default_harness, None);
        assert!(cfg.default_harness_model.is_empty());
        assert!(!cfg.updated_at.is_empty(), "seed must stamp updated_at");
    }

    #[tokio::test]
    async fn update_sets_and_reads_back_default_harness() {
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_harness: Some("opencode".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_harness.as_deref(), Some("opencode"));
        assert_eq!(
            get(&db).await.unwrap().default_harness.as_deref(),
            Some("opencode")
        );
        update(
            &db,
            UpdateInstanceConfig {
                default_harness: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(get(&db).await.unwrap().default_harness, None);
    }

    #[tokio::test]
    async fn update_sets_and_reads_back_per_harness_default_model() {
        let db = test_db().await;
        let map: std::collections::BTreeMap<String, String> = [
            ("claude".to_string(), "opus".to_string()),
            ("opencode".to_string(), "openrouter/foo".to_string()),
        ]
        .into_iter()
        .collect();
        update(
            &db,
            UpdateInstanceConfig {
                default_harness_model: Some(map.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(get(&db).await.unwrap().default_harness_model, map);
    }

    #[tokio::test]
    async fn init_migrates_pre_harness_schema() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap INTEGER,
                reaper_ttl_secs INTEGER,
                guard_timeout_secs INTEGER,
                default_model TEXT,
                triggers_paused INTEGER,
                default_sandbox TEXT,
                autocomplete_turn_end INTEGER,
                default_auto_name INTEGER,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 7, 'seed')",
        )
        .execute(&db)
        .await
        .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap();

        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.session_cap, Some(7), "pre-existing knob survives");
        assert_eq!(cfg.default_harness, None, "new column defaults to NULL");
        assert!(cfg.default_harness_model.is_empty());

        let map: std::collections::BTreeMap<String, String> =
            [("claude".to_string(), "opus".to_string())]
                .into_iter()
                .collect();
        update(
            &db,
            UpdateInstanceConfig {
                default_harness: Some("opencode".to_string()),
                default_harness_model: Some(map.clone()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.default_harness.as_deref(), Some("opencode"));
        assert_eq!(cfg.default_harness_model, map);
    }

    /// #563/ADR-0057: the Instance tier of `AgentChoice` — a fresh row carries
    /// none, round-trips a `Profile` reference, then clears back to `None` via
    /// the `Some(None)` double-Option idiom (mirrors `trigger_store::harness`).
    #[tokio::test]
    async fn agent_choice_round_trips_and_clears() {
        let db = test_db().await;
        assert_eq!(get(&db).await.unwrap().agent_choice, None);

        let choice = crate::agent_choice::AgentChoice::Profile {
            profile_id: "reviewer".to_string(),
        };
        let updated = update(
            &db,
            UpdateInstanceConfig {
                agent_choice: Some(Some(choice.clone())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.agent_choice, Some(choice.clone()));
        assert_eq!(get(&db).await.unwrap().agent_choice, Some(choice));

        // `Some(None)` clears it back to unset; `None` (the default) leaves it be.
        update(
            &db,
            UpdateInstanceConfig {
                agent_choice: Some(None),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(get(&db).await.unwrap().agent_choice, None);
    }

    /// #563: a `Custom` combination round-trips model/effort too, and an
    /// `agent_choice`-only edit does not disturb `default_harness`.
    #[tokio::test]
    async fn agent_choice_custom_round_trips_alongside_legacy_default_harness() {
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                default_harness: Some("claude".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let choice = crate::agent_choice::AgentChoice::Custom {
            harness: "opencode".to_string(),
            model: Some("gpt-5".to_string()),
            effort: Some("high".to_string()),
        };
        update(
            &db,
            UpdateInstanceConfig {
                agent_choice: Some(Some(choice.clone())),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.agent_choice, Some(choice));
        assert_eq!(
            cfg.default_harness.as_deref(),
            Some("claude"),
            "an agent_choice-only edit must not touch the legacy field"
        );
    }

    #[tokio::test]
    async fn init_migrates_pre_agent_choice_schema() {
        let db = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap INTEGER,
                reaper_ttl_secs INTEGER,
                guard_timeout_secs INTEGER,
                default_model TEXT,
                triggers_paused INTEGER,
                default_sandbox TEXT,
                autocomplete_turn_end INTEGER,
                default_auto_name INTEGER,
                default_harness TEXT,
                default_harness_model TEXT,
                auto_fail INTEGER,
                libassist_idle_ttl_secs INTEGER,
                updated_at TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 7, 'seed')",
        )
        .execute(&db)
        .await
        .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap();

        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.session_cap, Some(7), "pre-existing knob survives");
        assert_eq!(cfg.agent_choice, None, "new column defaults to NULL");

        let choice = crate::agent_choice::AgentChoice::Profile {
            profile_id: "reviewer".to_string(),
        };
        update(
            &db,
            UpdateInstanceConfig {
                agent_choice: Some(Some(choice.clone())),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(get(&db).await.unwrap().agent_choice, Some(choice));
    }

    #[tokio::test]
    async fn update_sets_default_model() {
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_model.as_deref(), Some("opus"));
        assert_eq!(
            get(&db).await.unwrap().default_model.as_deref(),
            Some("opus")
        );
        assert_eq!(updated.session_cap, None);
    }

    #[tokio::test]
    async fn update_clears_default_model_on_empty_string() {
        // "" is the clear sentinel: it must reset the column to NULL, not persist
        // an empty string (which would win precedence and reach the tail as an
        // empty `--model`).
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let cleared = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            cleared.default_model, None,
            "empty string must clear the stored model back to NULL"
        );
    }

    #[tokio::test]
    async fn default_model_only_edit_is_not_a_noop() {
        // Guard-rail for the `is_empty()` addition: an edit that touches ONLY
        // default_model (all numeric knobs None) must still write. Without the
        // `default_model.is_none()` clause it would fall into the no-op branch
        // and silently never persist.
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("haiku".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_model.as_deref(), Some("haiku"));
    }

    #[tokio::test]
    async fn update_sets_default_sandbox() {
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_sandbox: Some("minimal".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_sandbox.as_deref(), Some("minimal"));
        assert_eq!(
            get(&db).await.unwrap().default_sandbox.as_deref(),
            Some("minimal")
        );
        assert_eq!(updated.default_model, None);
    }

    #[tokio::test]
    async fn update_clears_default_sandbox_on_empty_string() {
        // "" is the clear sentinel: it resets the column to NULL (mirrors #347), so the
        // resolver falls back env→default(off).
        //
        // And the raw column really is SQL NULL, not `''`: a stored `''` would win precedence
        // and be treated as unset only by the empty-string filter downstream.
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                default_sandbox: Some("minimal".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let cleared = update(
            &db,
            UpdateInstanceConfig {
                default_sandbox: Some(String::new()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            cleared.default_sandbox, None,
            "empty string must clear the stored default sandbox back to NULL"
        );
        let raw: Option<String> =
            sqlx::query_scalar("SELECT default_sandbox FROM instance_config WHERE id = 1")
                .fetch_optional(&db)
                .await
                .unwrap()
                .flatten();
        assert_eq!(raw, None, "clear must persist NULL, never ''");
    }

    #[tokio::test]
    async fn default_sandbox_only_edit_is_not_a_noop() {
        // Guard-rail for the `is_empty()` addition: an edit touching ONLY
        // default_sandbox (all other knobs None) must still write.
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_sandbox: Some("full".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_sandbox.as_deref(), Some("full"));
    }

    #[tokio::test]
    async fn init_migrates_pre_default_sandbox_schema() {
        // Installs created before #410 lack the `default_sandbox` column. Simulate the
        // pre-#410 schema (also missing default_model + triggers_paused), then prove `init`
        // adds the column idempotently and it is writable afterwards, without clobbering the
        // stored knob. It is the LAST guarded ALTER left: #471 retired the two sandbox-image
        // columns, so a fresh table never grows them and an old one keeps them, inert.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 13, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap(); // idempotent

        let cfg = get(&db).await.unwrap();
        assert_eq!(
            cfg.session_cap,
            Some(13),
            "existing knob must survive the ALTER"
        );
        assert_eq!(cfg.default_sandbox, None, "new column defaults to NULL");

        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_sandbox: Some("minimal".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_sandbox.as_deref(), Some("minimal"));
    }

    /// A table created by TODAY's `init` has neither retired column — the reader must be silent,
    /// not error. This is the reference instance's case, and the only one an untouched install ever
    /// hits.
    #[tokio::test]
    async fn a_fresh_table_has_no_retired_sandbox_image_columns() {
        let db = test_db().await;
        let cols: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('instance_config')")
                .fetch_all(&db)
                .await
                .unwrap();
        for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
            assert!(
                !cols.contains(&column.to_string()),
                "#471: a fresh table must not grow `{column}` back; got {cols:?}"
            );
        }
        assert_eq!(retired_sandbox_image_values(&db).await.unwrap(), Vec::new());
    }

    /// A pre-#471 install: the columns exist and one carries a value the user typed. It must be
    /// FOUND — that is the whole point of AC5, a value that stops mattering never disappears in
    /// silence — and it must not resurface anywhere in [`InstanceConfig`].
    #[tokio::test]
    async fn a_stored_retired_value_is_found_but_is_not_config_any_more() {
        let db = pre_471_db().await;
        sqlx::query(
            "UPDATE instance_config SET image_source = 'dockerfile', \
             dockerfile_path = '/repo/docker/Dockerfile' WHERE id = 1",
        )
        .execute(&db)
        .await
        .unwrap();

        assert_eq!(
            retired_sandbox_image_values(&db).await.unwrap(),
            vec![
                ("image_source", "dockerfile".to_string()),
                ("dockerfile_path", "/repo/docker/Dockerfile".to_string()),
            ]
        );
        // `get` still works and carries no trace of either — the row is wider than the struct,
        // which is exactly what `SELECT *` + a narrower `row_to_config` buys.
        let cfg = get(&db).await.unwrap();
        assert_eq!(cfg.session_cap, Some(19), "the real knobs still read");
        let json = serde_json::to_value(&cfg).unwrap();
        for (column, _) in RETIRED_SANDBOX_IMAGE_COLUMNS {
            assert!(
                json.get(column).is_none(),
                "`{column}` must not come back through InstanceConfig: {json}"
            );
        }
    }

    /// `''` was the clear sentinel, so it never meant a value — reporting it would send the user
    /// hunting for a setting they had already cleared.
    #[tokio::test]
    async fn an_empty_retired_value_is_not_reported() {
        let db = pre_471_db().await;
        sqlx::query("UPDATE instance_config SET image_source = '' WHERE id = 1")
            .execute(&db)
            .await
            .unwrap();
        assert_eq!(retired_sandbox_image_values(&db).await.unwrap(), Vec::new());
    }

    /// A database as a pre-#471 daemon left it: the two retired columns present, one real knob set.
    /// Built by hand — `init` cannot produce this shape any more, which is the point.
    async fn pre_471_db() -> SqlitePool {
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                default_model      TEXT,
                triggers_paused    INTEGER,
                image_source       TEXT,
                default_sandbox    TEXT,
                dockerfile_path    TEXT,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 19, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();
        init(&db).await.unwrap(); // a real boot runs it; it must not drop the columns either
        db
    }

    #[tokio::test]
    async fn update_stores_autocomplete_turn_end_as_zero_or_one() {
        // #469: a bool on the wire, 0/1 in the column. Both directions PERSIST —
        // "off" must be a stored decision, not a fall-through, or unticking the
        // box could not override `PDO_AUTOCOMPLETE_TURN_END=1`.
        let db = test_db().await;

        let on = update(
            &db,
            UpdateInstanceConfig {
                autocomplete_turn_end: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(on.autocomplete_turn_end, Some(1));
        assert_eq!(get(&db).await.unwrap().autocomplete_turn_end, Some(1));

        let off = update(
            &db,
            UpdateInstanceConfig {
                autocomplete_turn_end: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            off.autocomplete_turn_end,
            Some(0),
            "unticking must persist a stored 0, never NULL"
        );
        assert_eq!(off.session_cap, None);
    }

    #[tokio::test]
    async fn autocomplete_turn_end_only_edit_is_not_a_noop() {
        // Guard-rail for the `is_empty()` addition: an edit touching ONLY this
        // knob must still write. Without the clause it would fall into the no-op
        // branch and the checkbox would silently never persist.
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                autocomplete_turn_end: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.autocomplete_turn_end, Some(1));
    }

    #[tokio::test]
    async fn init_migrates_pre_autocomplete_turn_end_schema() {
        // Installs created before #469 lack the column. Simulate that schema, then
        // prove `init` adds it idempotently, the existing knob survives, and the
        // feature defaults to OFF on an upgraded install (ADR-0012).
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 19, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap(); // idempotent

        let cfg = get(&db).await.unwrap();
        assert_eq!(
            cfg.session_cap,
            Some(19),
            "existing knob must survive the ALTER"
        );
        assert_eq!(
            cfg.autocomplete_turn_end, None,
            "an upgraded install must default to unset, i.e. OFF"
        );
        assert!(
            !crate::stale_detector::autocomplete_turn_end_with(cfg.autocomplete_turn_end)
                || crate::stale_detector::env_autocomplete_turn_end() == Some(true),
            "resolved OFF unless the env tier says otherwise"
        );

        let updated = update(
            &db,
            UpdateInstanceConfig {
                autocomplete_turn_end: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.autocomplete_turn_end, Some(1));
    }

    #[tokio::test]
    async fn update_stores_default_auto_name_as_zero_or_one() {
        // #338: a bool on the wire, 0/1 in the column. Both directions PERSIST —
        // "off" must be a stored decision, not a fall-through, or unticking the box
        // could not override `PDO_DEFAULT_AUTO_NAME=1`.
        let db = test_db().await;

        let on = update(
            &db,
            UpdateInstanceConfig {
                default_auto_name: Some(true),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(on.default_auto_name, Some(1));
        assert_eq!(get(&db).await.unwrap().default_auto_name, Some(1));

        let off = update(
            &db,
            UpdateInstanceConfig {
                default_auto_name: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            off.default_auto_name,
            Some(0),
            "unticking must persist a stored 0, never NULL"
        );
        assert_eq!(off.session_cap, None);
    }

    #[tokio::test]
    async fn default_auto_name_only_edit_is_not_a_noop() {
        // Guard-rail for the `is_empty()` addition: an edit touching ONLY this knob
        // must still write. Without the clause it would fall into the no-op branch
        // and the checkbox would silently never persist.
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_auto_name: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_auto_name, Some(0));
    }

    #[tokio::test]
    async fn init_migrates_pre_default_auto_name_schema() {
        // Installs created before #338 lack the column. Simulate that schema, then
        // prove `init` adds it idempotently, the existing knob survives, and an
        // upgraded install resolves to the pre-#338 behaviour (auto-name ON) unless
        // the env tier says otherwise.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 21, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap(); // idempotent

        let cfg = get(&db).await.unwrap();
        assert_eq!(
            cfg.session_cap,
            Some(21),
            "existing knob must survive the ALTER"
        );
        assert_eq!(
            cfg.default_auto_name, None,
            "an upgraded install must default to unset, i.e. fall through to env/default"
        );
        assert!(
            crate::prompt_augmenter::default_auto_name_with(cfg.default_auto_name)
                || crate::prompt_augmenter::env_default_auto_name() == Some(false),
            "resolved ON (pre-#338 behaviour) unless the env tier says otherwise"
        );

        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_auto_name: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_auto_name, Some(0));
    }

    #[tokio::test]
    async fn init_is_idempotent_and_never_clobbers_stored() {
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                session_cap: Some(7),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        init(&db).await.unwrap();
        assert_eq!(get(&db).await.unwrap().session_cap, Some(7));
    }

    #[tokio::test]
    async fn update_sets_only_the_given_fields() {
        let db = test_db().await;
        let updated = update(
            &db,
            UpdateInstanceConfig {
                session_cap: Some(4),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.session_cap, Some(4));
        assert_eq!(updated.reaper_ttl_secs, None);
        assert_eq!(updated.guard_timeout_secs, None);

        let updated = update(
            &db,
            UpdateInstanceConfig {
                reaper_ttl_secs: Some(120),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.session_cap, Some(4));
        assert_eq!(updated.reaper_ttl_secs, Some(120));
    }

    #[tokio::test]
    async fn update_bumps_updated_at() {
        let db = test_db().await;
        let before = get(&db).await.unwrap().updated_at;
        // now_iso() is millisecond-resolution; ensure a distinct instant.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        let after = update(
            &db,
            UpdateInstanceConfig {
                guard_timeout_secs: Some(30),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_ne!(after.updated_at, before, "a real edit must bump updated_at");
    }

    #[tokio::test]
    async fn empty_update_is_a_noop_and_leaves_updated_at() {
        let db = test_db().await;
        let before = get(&db).await.unwrap();
        let after = update(&db, UpdateInstanceConfig::default()).await.unwrap();
        assert_eq!(after, before, "a no-op edit must not mutate the row");
    }

    #[tokio::test]
    async fn get_is_idempotent() {
        let db = test_db().await;
        assert_eq!(get(&db).await.unwrap(), get(&db).await.unwrap());
    }

    #[tokio::test]
    async fn init_migrates_pre_default_model_schema() {
        // Existing installs created the table before #347, so it lacks the
        // `default_model` column. Simulate that old schema, then prove `init`'s
        // PRAGMA-guarded ALTER adds the column idempotently and preserves the
        // stored numeric knobs (no data loss).
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 7, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap();

        let cfg = get(&db).await.unwrap();
        assert_eq!(
            cfg.session_cap,
            Some(7),
            "existing knob must survive the ALTER"
        );
        assert_eq!(cfg.default_model, None, "new column defaults to NULL");

        let updated = update(
            &db,
            UpdateInstanceConfig {
                default_model: Some("opus".to_string()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(updated.default_model.as_deref(), Some("opus"));
    }

    #[tokio::test]
    async fn triggers_paused_defaults_off() {
        let db = test_db().await;
        assert!(!triggers_paused(&db).await.unwrap());
    }

    #[tokio::test]
    async fn set_triggers_paused_round_trips_and_clears_to_null() {
        // #348: pausing stores 1, unpausing stores NULL (not 0), and both are
        // observable through `triggers_paused`.
        let db = test_db().await;

        set_triggers_paused(&db, true).await.unwrap();
        assert!(triggers_paused(&db).await.unwrap(), "paused must read true");
        set_triggers_paused(&db, false).await.unwrap();
        assert!(
            !triggers_paused(&db).await.unwrap(),
            "unpaused must read false"
        );
        let raw: Option<i64> =
            sqlx::query_scalar("SELECT triggers_paused FROM instance_config WHERE id = 1")
                .fetch_optional(&db)
                .await
                .unwrap()
                .flatten();
        assert_eq!(raw, None, "unpause must persist NULL, never 0");
    }

    #[tokio::test]
    async fn triggers_paused_survives_init_and_is_orthogonal_to_settings() {
        // The kill-switch persists across a daemon restart (a second `init`) and
        // does not disturb the `/settings` knobs, nor they it (#348: two
        // orthogonal channels sharing one row).
        let db = test_db().await;
        update(
            &db,
            UpdateInstanceConfig {
                session_cap: Some(5),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        set_triggers_paused(&db, true).await.unwrap();

        init(&db).await.unwrap(); // mimic a restart

        assert!(
            triggers_paused(&db).await.unwrap(),
            "pause flag must survive a restart"
        );
        assert_eq!(
            get(&db).await.unwrap().session_cap,
            Some(5),
            "a settings knob must survive the pause write untouched"
        );
    }

    #[tokio::test]
    async fn init_migrates_pre_triggers_paused_schema() {
        // Installs created before #348 lack the `triggers_paused` column. Simulate
        // that schema (also missing default_model, so this covers the double
        // guarded ALTER), then prove `init` adds the column idempotently and the
        // flag is writable afterwards, without clobbering the stored knob.
        let db = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE instance_config (
                id                 INTEGER PRIMARY KEY CHECK (id = 1),
                session_cap        INTEGER,
                reaper_ttl_secs    INTEGER,
                guard_timeout_secs INTEGER,
                updated_at         TEXT NOT NULL
            )",
        )
        .execute(&db)
        .await
        .unwrap();
        sqlx::query("INSERT INTO instance_config (id, session_cap, updated_at) VALUES (1, 9, ?)")
            .bind(crate::event_log::now_iso())
            .execute(&db)
            .await
            .unwrap();

        init(&db).await.unwrap();
        init(&db).await.unwrap(); // idempotent

        assert!(
            !triggers_paused(&db).await.unwrap(),
            "migrated column defaults to not-paused"
        );
        assert_eq!(
            get(&db).await.unwrap().session_cap,
            Some(9),
            "existing knob must survive the ALTER"
        );
        set_triggers_paused(&db, true).await.unwrap();
        assert!(
            triggers_paused(&db).await.unwrap(),
            "the migrated column is writable"
        );
    }
}
