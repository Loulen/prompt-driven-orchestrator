//! Staging profiles — the named, editable content of a sandboxed Run's staged home
//! (#432, slice K of PRD #403; ADR-0031 §2-§7).
//!
//! Before this module, what a sandboxed Run carried in its home was an invisible Rust
//! constant: two arrays at the bottom of [`crate::sandbox_staging`]. You could neither
//! see it, change it, nor have two versions of it. A *staging profile* replaces that
//! two-position switch with a **named list**.
//!
//! Shape mirrors [`crate::instance_config`]: the **pure** functions first (classes,
//! defaults, validation, resolution, the [`landing`] classifier), the sqlx CRUD below.
//! Nothing here reads `$HOME`, shells out, or touches Docker.
//!
//! ## The three load-bearing ideas
//!
//! 1. **A profile is a *diff*, never a snapshot** (ADR-0031 §2). The row stores the
//!    user's intention — `disabled` / `extras` — so the day a PDO release adds an entry
//!    to the default, every existing profile sees it. A snapshot would freeze the
//!    install forever. Corollary: `full` and `minimal` are **virtual defaults** with no
//!    row at all until edited, and editing one materialises a row that *also* holds a
//!    diff.
//!
//! 2. **One classifier, two views.** [`landing`] decides, for a single entry, where it
//!    lands in the staging — and it is called by BOTH the copy view
//!    ([`crate::sandbox_staging::prepare`]) and the mount view
//!    ([`crate::sandbox_staging::extra_mounts`]). They therefore cannot drift, and the
//!    "an entry under `.claude/` produces no extra `-v`" dedup of ADR-0031 §4 is not a
//!    special case: it is a *consequence*.
//!
//! 3. **The floor is not editable, and not an entry either** (ADR-0031 §1). The five
//!    guarantees [`crate::sandbox_staging`] holds in every profile are satisfied either
//!    by an entry or by a fallback synthesis. Two of them need *keys* in a file the
//!    profile may also carry (class **(b)** below) — unchecking those is safe. Three
//!    need the *whole* file, so they are class **(c)**: shown read-only, never
//!    checkable, and **refused as extras** too.
//!
//! ## No rename in v1
//!
//! `name` is the primary key AND the value stored by all three consumers
//! (`triggers.sandbox`, `instance_config.default_sandbox`, the frozen `RunStarted`
//! payload). Renaming is therefore *delete + create*. A future rename must be a
//! **repointing transaction** across those three stores — never an `UPDATE` of this key.

use anyhow::Result;
use serde::Serialize;
use sqlx::{Row, SqlitePool};

// -- vocabulary --------------------------------------------------------------

/// The virtual default that carries the full replica of the host `~/.claude`.
pub(crate) const FULL_PROFILE: &str = "full";
/// The virtual default that carries **nothing** in its own right: `minimal` *is* the
/// staging floor, which is exactly the empty entry list.
pub(crate) const MINIMAL_PROFILE: &str = "minimal";

/// The two names that resolve with no database row (ADR-0031 §2). Creating one *is*
/// materialising it, which is why [`validate_profile_name`] allows them.
pub(crate) const VIRTUAL_PROFILES: &[&str] = &[FULL_PROFILE, MINIMAL_PROFILE];

/// Longest accepted profile name. The name is a URL segment, a NOCASE index key, a
/// `<select>` value and an equality token compared across three stores and one
/// immutable event — so it stays short, ASCII and boring.
pub(crate) const MAX_PROFILE_NAME_LEN: usize = 32;

/// `$HOME`-relative prefixes we stage happily but flag in the UI (ADR-0031 §3).
/// Forbidding them would be theatre while ADR-0030 already assumes the host uid, the
/// repo mounted rw and real Claude credentials.
pub(crate) const SENSITIVE_PREFIXES: &[&str] = &[".ssh", ".aws", ".gnupg"];

/// What an entry points at, for the editor's right-hand column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EntryKind {
    Dir,
    File,
    /// A one-level filename pattern (only `.claude/*.md` ships as one). Users cannot
    /// author these — [`validate_entry`] rejects `*` — so the pattern in a resolved
    /// list can only have come from the default.
    Glob,
}

/// One entry of the built-in default (the `full` profile's 9 lines, ADR-0031 §2).
pub(crate) struct DefaultEntry {
    /// `$HOME`-relative path or one-level glob.
    pub path: &'static str,
    pub kind: EntryKind,
    /// Class **(b)**: unchecking it does NOT make the file absent — the staging floor
    /// re-synthesises the keys it needs. Exactly two entries, and the UI must say so,
    /// or unchecking looks more destructive than it is.
    pub resynthesised: bool,
    /// Static, server-owned advisory shown under the entry. Static on purpose: a real
    /// recursive size walk of `plugins/**/node_modules` is seconds of IO, and the
    /// settings handler explicitly budgets against that (#373 discipline — the daemon
    /// owns every derived label, the client computes none).
    pub note: Option<&'static str>,
}

/// The `full` profile — the built-in default every profile except `minimal` diffs
/// against. Order here is the *editor's* reading order; [`resolve_entry_list`] sorts.
///
/// `.credentials.json` is deliberately absent: it is a **floor** guarantee (G1), and
/// having it in both places made the constant lie about being a list of entries.
pub(crate) const DEFAULT_FULL_ENTRIES: &[DefaultEntry] = &[
    DefaultEntry {
        path: ".claude.json",
        kind: EntryKind::File,
        resynthesised: true,
        note: Some(
            "Unchecked, onboarding and folder trust are synthesised instead — your \
             oauthAccount / emailAddress never leave the host.",
        ),
    },
    DefaultEntry {
        path: ".claude/*.md",
        kind: EntryKind::Glob,
        resynthesised: false,
        note: Some("CLAUDE.md and its top-level imports (RTK.md, …)."),
    },
    DefaultEntry {
        path: ".claude/settings.json",
        kind: EntryKind::File,
        resynthesised: true,
        note: Some("Unchecked, a one-key settings.json is synthesised instead — not absent."),
    },
    DefaultEntry {
        path: ".claude/settings.local.json",
        kind: EntryKind::File,
        resynthesised: false,
        note: None,
    },
    DefaultEntry {
        path: ".claude/agents",
        kind: EntryKind::Dir,
        resynthesised: false,
        note: None,
    },
    DefaultEntry {
        path: ".claude/commands",
        kind: EntryKind::Dir,
        resynthesised: false,
        note: None,
    },
    DefaultEntry {
        path: ".claude/output-styles",
        kind: EntryKind::Dir,
        resynthesised: false,
        note: None,
    },
    DefaultEntry {
        path: ".claude/plugins",
        kind: EntryKind::Dir,
        resynthesised: false,
        note: Some("≈1 GB per run, dominated by plugins/*/node_modules."),
    },
    DefaultEntry {
        path: ".claude/skills",
        kind: EntryKind::Dir,
        resynthesised: false,
        note: Some("May pull targets from outside ~/.claude (links into ~/.agents)."),
    },
];

/// One class-**(c)** floor guarantee: satisfied by the *whole* file, so it is neither
/// checkable nor addable. Shown read-only in the editor because without that block a
/// `minimal` profile's screen looks broken and the user wrongly concludes the container
/// starts with no credentials.
pub(crate) struct FloorGuarantee {
    pub id: &'static str,
    /// What the container gets, in the user's words.
    pub label: &'static str,
    /// The `$HOME`-relative path the floor owns, when there is one.
    pub path: Option<&'static str>,
}

/// The staging floor, verbatim from [`crate::sandbox_staging::enforce_staging_floor`].
/// G3/G4 appear here as guarantees even though their files are class-(b) *entries* —
/// the difference is the rule that decides the classes: **(b) = the floor needs keys in
/// the file; (c) = the floor needs the file whole.**
pub(crate) const FLOOR_GUARANTEES: &[FloorGuarantee] = &[
    FloorGuarantee {
        id: "credentials",
        label: "Valid Claude credentials",
        path: Some(".claude/.credentials.json"),
    },
    FloorGuarantee {
        id: "org-managed-settings",
        label: "Your organisation's managed settings, consented",
        path: Some(".claude/remote-settings.json"),
    },
    FloorGuarantee {
        id: "bypass-permissions",
        label: "Permissions bypass accepted (no blocking dialog)",
        path: None,
    },
    FloorGuarantee {
        id: "run-root-trust",
        label: "Trust pre-granted on the Run's repo root",
        path: None,
    },
    FloorGuarantee {
        id: "empty-projects",
        label: "An empty projects/ transcript sink",
        path: Some(".claude/projects"),
    },
];

/// Paths the floor owns **whole** — refused as extras, never checkable.
const FLOOR_OWNED_PATHS: &[&str] = &[
    ".claude/.credentials.json",
    ".claude/remote-settings.json",
    ".claude/projects",
];

// -- pure: the classifier ----------------------------------------------------

/// Where an entry lands in the staging. The SINGLE classifier shared by the copy view
/// and the mount view (see the module header, idea 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Landing<'a> {
    /// Under `.claude/` → `<staging>/claude-home/<rel>`, already served by the fixed
    /// `.claude` mount. Produces **no** `-v` of its own (ADR-0031 §4 dedup).
    ClaudeHome { rel: &'a str, glob: bool },
    /// `.claude.json` → the `<staging>/.claude.json` **sibling**, served by fixed
    /// mount #3. It must NOT land under `claude-home/`: there it would appear at
    /// `$HOME/.claude/.claude.json`, where Claude Code never looks.
    ClaudeJson,
    /// Anything else under `$HOME` → copied to `<staging>/home/<rel>` and bind-mounted
    /// rw at `$HOME/<rel>`. Never a direct bind of the host file (ADR-0031 §4).
    HomeExtra { rel: &'a str },
}

/// Classify one `$HOME`-relative entry. Pure path math, no IO.
pub(crate) fn landing(entry: &str) -> Landing<'_> {
    if entry == ".claude.json" {
        return Landing::ClaudeJson;
    }
    if let Some(rel) = entry.strip_prefix(".claude/") {
        return Landing::ClaudeHome {
            rel,
            glob: rel.contains('*'),
        };
    }
    Landing::HomeExtra { rel: entry }
}

// -- pure: names -------------------------------------------------------------

/// Grammar: `^[a-z0-9][a-z0-9-]{0,31}$`, trimmed first then checked **strictly**.
///
/// - `off` is **reserved**: it short-circuits resolution before any lookup, so a
///   profile named `off` would be unreachable for life.
/// - `""` is **reserved**: it is the *clear* sentinel in `put_settings`, trigger create
///   and `default_sandbox_with`.
/// - `full` / `minimal` are **allowed** — creating them *is* materialising them.
/// - An uppercase name is a **400, never folded**: accepting `Foo` and storing `foo`
///   would have the UI hunt through a list that displays `foo`. See the deliberate
///   asymmetry documented on [`crate::event_log::SandboxMode::parse`].
pub(crate) fn validate_profile_name(raw: &str) -> Result<String, String> {
    let name = raw.trim();
    if name.is_empty() {
        return Err("a profile name cannot be blank".to_string());
    }
    if name.eq_ignore_ascii_case(crate::event_log::SandboxMode::OFF_WIRE) {
        return Err("`off` is reserved: it means \"no sandbox\", not a profile".to_string());
    }
    if name.len() > MAX_PROFILE_NAME_LEN {
        return Err(format!(
            "profile name `{name}` is longer than {MAX_PROFILE_NAME_LEN} characters"
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().expect("non-empty checked above");
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return Err(format!(
            "profile name `{name}` must start with a lowercase letter or a digit"
        ));
    }
    if !chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') {
        return Err(format!(
            "profile name `{name}` may only contain lowercase letters, digits and `-`"
        ));
    }
    Ok(name.to_string())
}

// -- pure: entries -----------------------------------------------------------

/// Validate and **normalise** one user-authored entry (an *extra*).
///
/// Returns the normalised form — `./foo` → `foo`, `foo/` → `foo`, `foo//bar` →
/// `foo/bar` — which is what makes dedup and the ancestor collapse well defined.
///
/// The insight that lets this be pure, with no `$HOME` in scope: once an absolute path
/// and `..` are refused, a relative path **cannot** leave its root lexically, so
/// knowing `$HOME` would add nothing. Escaping by **symlink** is not decidable here at
/// all (the path may not exist yet; the tree mutates between this write and `prepare`),
/// and the answer already lives where it belongs — the `copy_root` deref of
/// `copy_tree_preserving`.
pub(crate) fn validate_entry(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("an entry cannot be blank".to_string());
    }
    if trimmed.contains('\0') {
        return Err("an entry cannot contain a NUL byte".to_string());
    }
    // A backslash is a LEGAL filename character under Linux, so `..\..\x` would be ONE
    // component and would sail straight through the `..` test below.
    if trimmed.contains('\\') {
        return Err(format!("`{trimmed}`: an entry cannot contain a backslash"));
    }
    if trimmed.contains(['*', '?', '[']) {
        return Err(format!(
            "`{trimmed}`: patterns are not accepted — pick the file or folder itself"
        ));
    }
    if trimmed.starts_with('/') {
        return Err(format!(
            "`{trimmed}`: an entry is relative to $HOME, not an absolute path"
        ));
    }
    let mut parts: Vec<&str> = Vec::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return Err(format!("`{trimmed}`: `..` would escape $HOME")),
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return Err(format!("`{trimmed}`: that is $HOME itself, not an entry"));
    }
    let norm = parts.join("/");

    // Three hard refusals, each of which would otherwise kill EVERY Run of the profile
    // with an opaque `Error response from daemon: Duplicate mount point`, or leave an
    // undeletable staging behind.
    if norm == ".claude" {
        return Err(
            "`.claude`: the staged Claude home is already mounted whole — list the \
             entries you want under it instead"
                .to_string(),
        );
    }
    if norm == ".pdo" || norm.starts_with(".pdo/") {
        return Err(format!(
            "`{norm}`: `.pdo` holds the staging root itself — staging it would copy \
             every other Run's staging into this one"
        ));
    }
    if FLOOR_OWNED_PATHS.contains(&norm.as_str())
        || FLOOR_OWNED_PATHS
            .iter()
            .any(|p| norm.starts_with(&format!("{p}/")))
    {
        return Err(format!(
            "`{norm}`: the staging floor owns that path in every profile — it is \
             guaranteed, not selectable"
        ));
    }
    Ok(norm)
}

/// The built-in default a profile diffs against: **empty** for `minimal` (which *is*
/// the floor), the `full` list for every other name.
///
/// So a brand-new profile starts as a copy of `full` with everything checked — which is
/// what makes "uncheck `.claude/plugins`" the two-click operation the issue asks for.
pub(crate) fn base_entries(name: &str) -> Vec<&'static str> {
    if name == MINIMAL_PROFILE {
        Vec::new()
    } else {
        DEFAULT_FULL_ENTRIES.iter().map(|e| e.path).collect()
    }
}

/// Outcome of folding a stored diff over a base list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedEntries {
    /// `base ∪ extras − disabled`, normalised, sorted, deduplicated, ancestor-collapsed.
    /// This is the list frozen into `RunStarted`.
    pub entries: Vec<String>,
    /// Extras that the base already carries. A **signalled no-op**, not an error: the
    /// default may *lose* the entry tomorrow, and rejecting it would be snapshot
    /// thinking.
    pub redundant_extras: Vec<String>,
    /// `disabled` names absent from the base. Also a signalled no-op — this is
    /// ADR-0031 §2 verbatim (unchecking `plugins` before your PDO version has it).
    pub inactive_disabled: Vec<String>,
}

/// Fold a diff over a base list. **Pure, total, no IO** — the single resolver shared by
/// the create-run chokepoint, the boot-recovery replay and the editor's read model, so
/// what the UI shows can never drift from what gets staged (#373).
pub(crate) fn resolve_entry_list(
    base: &[&str],
    disabled: &[String],
    extras: &[String],
) -> ResolvedEntries {
    let inactive_disabled: Vec<String> = disabled
        .iter()
        .filter(|d| !base.contains(&d.as_str()))
        .cloned()
        .collect();
    let redundant_extras: Vec<String> = extras
        .iter()
        .filter(|e| base.contains(&e.as_str()))
        .cloned()
        .collect();

    let mut entries: Vec<String> = base
        .iter()
        .filter(|b| !disabled.iter().any(|d| d == *b))
        .map(|b| b.to_string())
        .collect();
    for extra in extras {
        if !entries.iter().any(|e| e == extra) {
            entries.push(extra.clone());
        }
    }
    // Sort BEFORE collapsing — not cosmetics. A proper ancestor always sorts before its
    // descendants, which makes the collapse a single linear pass and removes the whole
    // "someone reached for a HashSet in the normaliser" error class.
    entries.sort();
    entries.dedup();
    entries = collapse_nested(entries);

    ResolvedEntries {
        entries,
        redundant_extras,
        inactive_disabled,
    }
}

/// Drop every entry that has a proper ancestor in the (sorted) list: `.config` +
/// `.config/gh` → `.config` alone. Without it Docker would refuse the container with
/// `Duplicate mount point`, or silently resolve the pair by destination depth.
fn collapse_nested(sorted: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(sorted.len());
    for entry in sorted {
        let shadowed = out
            .iter()
            .any(|kept| entry.starts_with(&format!("{kept}/")));
        if !shadowed {
            out.push(entry);
        }
    }
    out
}

// -- store: the persisted diff ------------------------------------------------

/// A materialised profile row: the user's **intention**, never the effective list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ProfileDiff {
    pub name: String,
    pub disabled: Vec<String>,
    pub extras: Vec<String>,
    pub updated_at: String,
}

/// A profile as the API serves it: the stored diff **plus** what it resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedProfile {
    pub name: String,
    /// One of [`VIRTUAL_PROFILES`] — i.e. it resolves with or without a row.
    pub is_virtual: bool,
    /// A row exists (the profile has been edited at least once).
    pub materialised: bool,
    pub disabled: Vec<String>,
    pub extras: Vec<String>,
    pub resolved: ResolvedEntries,
    pub updated_at: Option<String>,
}

/// Create the `sandbox_profiles` table if absent. A brand-new table, so
/// `CREATE TABLE IF NOT EXISTS` **is** the whole migration — there is nothing to
/// `ALTER`. Future columns go through the idempotent PRAGMA-guarded
/// `ALTER TABLE … ADD COLUMN` idiom (precedent: `max_concurrent`, #239 in
/// [`crate::trigger_store`]), **never** a migration runner.
///
/// **No seed row**: `minimal` and `full` are virtual (ADR-0031 §2).
///
/// `COLLATE NOCASE` is belt-and-braces — [`validate_profile_name`] makes a case
/// collision unreachable, but if a validator is ever loosened the index refuses the
/// duplicate instead of creating two profiles that display identically.
pub(crate) async fn init(db: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sandbox_profiles (
            name       TEXT PRIMARY KEY COLLATE NOCASE,
            disabled   JSON NOT NULL DEFAULT '[]',
            extras     JSON NOT NULL DEFAULT '[]',
            updated_at TEXT NOT NULL
        )",
    )
    .execute(db)
    .await?;
    Ok(())
}

/// Two columns rather than one `diff` blob: they validate differently, they play
/// opposite roles at resolution (subtract vs add), and a corrupt one degrades to
/// `Vec::new()` on its own. Precedent: `variables JSON` in
/// [`crate::trigger_store`].
fn row_to_diff(row: &sqlx::sqlite::SqliteRow) -> ProfileDiff {
    let disabled: String = row.get("disabled");
    let extras: String = row.get("extras");
    ProfileDiff {
        name: row.get("name"),
        disabled: serde_json::from_str(&disabled).unwrap_or_default(),
        extras: serde_json::from_str(&extras).unwrap_or_default(),
        updated_at: row.get("updated_at"),
    }
}

/// The stored diff for `name`, or `None` when the profile has never been edited.
pub(crate) async fn get_diff(
    db: &SqlitePool,
    name: &str,
) -> Result<Option<ProfileDiff>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT name, disabled, extras, updated_at FROM sandbox_profiles WHERE name = ?",
    )
    .bind(name)
    .fetch_optional(db)
    .await?;
    Ok(row.as_ref().map(row_to_diff))
}

/// Every materialised diff, by name.
pub(crate) async fn list_diffs(db: &SqlitePool) -> Result<Vec<ProfileDiff>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT name, disabled, extras, updated_at FROM sandbox_profiles ORDER BY name",
    )
    .fetch_all(db)
    .await?;
    Ok(rows.iter().map(row_to_diff).collect())
}

/// `upsert`, not create-then-update: the caller cannot know whether `full` is already
/// materialised, and ADR-0031 §2 says editing it *is* what materialises it.
pub(crate) async fn upsert(
    db: &SqlitePool,
    name: &str,
    disabled: &[String],
    extras: &[String],
) -> Result<ProfileDiff, sqlx::Error> {
    let now = crate::event_log::now_iso();
    let disabled_json = serde_json::to_string(disabled).unwrap_or_else(|_| "[]".to_string());
    let extras_json = serde_json::to_string(extras).unwrap_or_else(|_| "[]".to_string());
    sqlx::query(
        "INSERT INTO sandbox_profiles (name, disabled, extras, updated_at)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(name) DO UPDATE SET
            disabled = excluded.disabled,
            extras = excluded.extras,
            updated_at = excluded.updated_at",
    )
    .bind(name)
    .bind(&disabled_json)
    .bind(&extras_json)
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
pub(crate) async fn delete(db: &SqlitePool, name: &str) -> Result<bool, sqlx::Error> {
    let res = sqlx::query("DELETE FROM sandbox_profiles WHERE name = ?")
        .bind(name)
        .execute(db)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Resolve `name` to its effective entry list, or `None` when no such profile exists.
///
/// This is the ONE existence oracle: `Ok(None)` is what every edge turns into a hard
/// failure (400 at create, a red Trigger fire record, `RunFailed` at boot recovery) so
/// an unknown name can never fall back silently to less isolation (ADR-0031 §7).
pub(crate) async fn resolve(
    db: &SqlitePool,
    name: &str,
) -> Result<Option<ResolvedProfile>, sqlx::Error> {
    let stored = get_diff(db, name).await?;
    let is_virtual = VIRTUAL_PROFILES.contains(&name);
    let Some(diff) = stored else {
        if !is_virtual {
            return Ok(None);
        }
        let base = base_entries(name);
        return Ok(Some(ResolvedProfile {
            name: name.to_string(),
            is_virtual,
            materialised: false,
            disabled: Vec::new(),
            extras: Vec::new(),
            resolved: resolve_entry_list(&base, &[], &[]),
            updated_at: None,
        }));
    };
    // The stored row's own `name` casing wins — it is what the PK holds.
    let base = base_entries(&diff.name);
    let resolved = resolve_entry_list(&base, &diff.disabled, &diff.extras);
    Ok(Some(ResolvedProfile {
        name: diff.name,
        is_virtual,
        materialised: true,
        disabled: diff.disabled,
        extras: diff.extras,
        resolved,
        updated_at: Some(diff.updated_at),
    }))
}

/// Whether `name` names a resolvable profile. Thin wrapper over [`resolve`] so the
/// existence rule has exactly one implementation.
pub(crate) async fn exists(db: &SqlitePool, name: &str) -> Result<bool, sqlx::Error> {
    Ok(resolve(db, name).await?.is_some())
}

/// Every profile name the instance can serve: the two virtual defaults ∪ the
/// materialised rows, deduplicated (an edited `full` appears once), sorted.
pub(crate) async fn list_names(db: &SqlitePool) -> Result<Vec<(String, bool)>, sqlx::Error> {
    let mut names: Vec<String> = VIRTUAL_PROFILES.iter().map(|s| s.to_string()).collect();
    for diff in list_diffs(db).await? {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&diff.name)) {
            names.push(diff.name);
        }
    }
    names.sort();
    Ok(names
        .into_iter()
        .map(|n| {
            let v = VIRTUAL_PROFILES.contains(&n.as_str());
            (n, v)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // -- the golden that anchors the whole slice -----------------------------

    /// `resolve_entry_list(full, [], [])` must equal what the pre-#432 constant staged,
    /// entry for entry. This is the one test that proves "profiles" did not silently
    /// change the default staging.
    #[test]
    fn full_default_resolves_to_the_historical_allowlist() {
        let base = base_entries(FULL_PROFILE);
        let got = resolve_entry_list(&base, &[], &[]);
        assert_eq!(
            got.entries,
            v(&[
                ".claude.json",
                ".claude/*.md",
                ".claude/agents",
                ".claude/commands",
                ".claude/output-styles",
                ".claude/plugins",
                ".claude/settings.json",
                ".claude/settings.local.json",
                ".claude/skills",
            ]),
        );
        assert!(got.redundant_extras.is_empty());
        assert!(got.inactive_disabled.is_empty());
    }

    /// `minimal` IS the floor: the empty list, not an error and not a special case.
    #[test]
    fn minimal_default_resolves_to_the_empty_list() {
        let base = base_entries(MINIMAL_PROFILE);
        assert!(base.is_empty());
        assert!(resolve_entry_list(&base, &[], &[]).entries.is_empty());
    }

    // -- landing(): the single classifier ------------------------------------

    /// GOLDEN, load-bearing: `.claude.json` is the SIBLING, never under `claude-home/`.
    /// Filed there it would surface at `$HOME/.claude/.claude.json`, where Claude Code
    /// never looks — see the note on
    /// [`crate::sandbox_staging::staged_claude_json`]. Do not "tidy" this.
    #[test]
    fn landing_routes_claude_json_to_the_sibling() {
        assert_eq!(landing(".claude.json"), Landing::ClaudeJson);
    }

    #[test]
    fn landing_routes_under_claude_to_the_staged_home() {
        assert_eq!(
            landing(".claude/skills"),
            Landing::ClaudeHome {
                rel: "skills",
                glob: false
            }
        );
        assert_eq!(
            landing(".claude/*.md"),
            Landing::ClaudeHome {
                rel: "*.md",
                glob: true
            }
        );
    }

    #[test]
    fn landing_routes_everything_else_to_a_home_extra() {
        assert_eq!(
            landing(".gitconfig"),
            Landing::HomeExtra { rel: ".gitconfig" }
        );
        assert_eq!(
            landing(".config/gh"),
            Landing::HomeExtra { rel: ".config/gh" }
        );
        // A path that merely *starts with* the same bytes is NOT under `.claude/`.
        assert_eq!(
            landing(".claudex/thing"),
            Landing::HomeExtra {
                rel: ".claudex/thing"
            }
        );
    }

    // -- resolve_entry_list --------------------------------------------------

    #[test]
    fn disabling_an_entry_removes_it() {
        let base = base_entries(FULL_PROFILE);
        let got = resolve_entry_list(&base, &v(&[".claude/plugins"]), &[]);
        assert!(!got.entries.iter().any(|e| e == ".claude/plugins"));
        assert_eq!(got.entries.len(), DEFAULT_FULL_ENTRIES.len() - 1);
        assert!(got.inactive_disabled.is_empty());
    }

    #[test]
    fn extras_are_added_and_sorted_in() {
        let base = base_entries(FULL_PROFILE);
        let got = resolve_entry_list(&base, &[], &v(&[".gitconfig", ".config/gh"]));
        // `.config/gh` sorts before `.gitconfig`, and both after the `.claude*` block.
        assert_eq!(got.entries[0], ".claude.json");
        assert!(got.entries.contains(&".config/gh".to_string()));
        assert!(got.entries.contains(&".gitconfig".to_string()));
        let sorted = {
            let mut s = got.entries.clone();
            s.sort();
            s
        };
        assert_eq!(got.entries, sorted, "the resolved list is sorted");
    }

    /// An extra already in the default is a SIGNALLED no-op, never an error: the
    /// default may lose the entry tomorrow. Snapshot thinking is the bug here.
    #[test]
    fn a_redundant_extra_is_flagged_not_rejected() {
        let base = base_entries(FULL_PROFILE);
        let got = resolve_entry_list(&base, &[], &v(&[".claude/skills"]));
        assert_eq!(got.redundant_extras, v(&[".claude/skills"]));
        assert_eq!(got.entries.len(), DEFAULT_FULL_ENTRIES.len());
    }

    /// ADR-0031 §2 verbatim: unchecking `plugins` on a PDO version whose default does
    /// not carry it yet must be REMEMBERED, so the day it lands the profile still says
    /// no.
    #[test]
    fn an_inactive_disabled_is_flagged_not_rejected() {
        let got = resolve_entry_list(&[".claude/skills"], &v(&[".claude/plugins"]), &[]);
        assert_eq!(got.inactive_disabled, v(&[".claude/plugins"]));
        assert_eq!(got.entries, v(&[".claude/skills"]));
    }

    #[test]
    fn nested_entries_collapse_onto_their_ancestor() {
        let got = resolve_entry_list(&[], &[], &v(&[".config/gh", ".config", ".configuration"]));
        // `.config` swallows `.config/gh`; `.configuration` is NOT a descendant (the
        // check is component-wise, not a bare `starts_with`).
        assert_eq!(got.entries, v(&[".config", ".configuration"]));
    }

    #[test]
    fn duplicate_extras_are_deduplicated() {
        let got = resolve_entry_list(&[], &[], &v(&[".gitconfig", ".gitconfig"]));
        assert_eq!(got.entries, v(&[".gitconfig"]));
    }

    // -- validate_profile_name ----------------------------------------------

    #[test]
    fn profile_names_accept_the_grammar() {
        for ok in ["full", "minimal", "full-no-mcp", "a", "0", "x9-y"] {
            assert_eq!(
                validate_profile_name(ok).unwrap(),
                ok,
                "{ok} should be valid"
            );
        }
        assert_eq!(validate_profile_name("  full  ").unwrap(), "full");
    }

    #[test]
    fn profile_names_reject_the_reserved_and_the_malformed() {
        for bad in [
            "",
            "   ",
            "off",
            "OFF",
            "Foo",
            "-lead",
            "under_score",
            "spa ce",
            "é",
            "with.dot",
        ] {
            assert!(
                validate_profile_name(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
        let too_long = "a".repeat(MAX_PROFILE_NAME_LEN + 1);
        assert!(validate_profile_name(&too_long).is_err());
        assert!(validate_profile_name(&"a".repeat(MAX_PROFILE_NAME_LEN)).is_ok());
    }

    // -- validate_entry ------------------------------------------------------

    #[test]
    fn entries_normalise_to_a_canonical_relative_path() {
        assert_eq!(validate_entry("./foo").unwrap(), "foo");
        assert_eq!(validate_entry("foo/").unwrap(), "foo");
        assert_eq!(validate_entry("foo//bar").unwrap(), "foo/bar");
        assert_eq!(validate_entry("  .gitconfig  ").unwrap(), ".gitconfig");
        assert_eq!(validate_entry(".config/gh").unwrap(), ".config/gh");
        // Sensitive but ALLOWED (ADR-0031 §3) — warned in the UI, not refused.
        assert_eq!(validate_entry(".ssh").unwrap(), ".ssh");
    }

    #[test]
    fn entries_reject_escapes_and_reserved_paths() {
        for bad in [
            "",
            "   ",
            ".",
            "./",
            "/etc/passwd",
            "../x",
            "foo/../../x",
            // A backslash is a legal Linux filename char, so this would otherwise be
            // ONE component and pass the `..` test.
            "..\\..\\x",
            ".claude/*.md",
            "foo?",
            ".claude",
            ".pdo",
            ".pdo/sandbox",
            ".claude/projects",
            ".claude/projects/-enc/x.jsonl",
            ".claude/.credentials.json",
            ".claude/remote-settings.json",
        ] {
            assert!(validate_entry(bad).is_err(), "{bad:?} should be rejected");
        }
        assert!(validate_entry("a\0b").is_err());
    }

    /// The default's glob is authored HERE, never by a user — which is why
    /// `validate_entry` may refuse `*` while the resolved list still carries `*.md`.
    #[test]
    fn the_default_glob_is_not_user_authorable() {
        assert!(base_entries(FULL_PROFILE).contains(&".claude/*.md"));
        assert!(validate_entry(".claude/*.md").is_err());
    }

    /// Exactly two class-(b) entries — the floor re-synthesises those two files, so
    /// unchecking them is safe. The UI copy depends on this count being right.
    #[test]
    fn exactly_two_default_entries_are_resynthesised_by_the_floor() {
        let resynth: Vec<&str> = DEFAULT_FULL_ENTRIES
            .iter()
            .filter(|e| e.resynthesised)
            .map(|e| e.path)
            .collect();
        assert_eq!(resynth, vec![".claude.json", ".claude/settings.json"]);
    }
}
