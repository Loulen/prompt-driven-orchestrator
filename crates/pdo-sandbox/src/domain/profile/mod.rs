//! Staging profiles — the named, editable content of a sandboxed Run's staged home
//! (#432, slice K of PRD #403; ADR-0031 §2-§7).
//!
//! Before profiles, what a sandboxed Run carried in its home was an invisible Rust
//! constant: two arrays at the bottom of the staging code. You could neither see it,
//! change it, nor have two versions of it. A *staging profile* replaces that
//! two-position switch with a **named list**.
//!
//! This module is the **pure** half: classes, defaults, validation, resolution, the
//! [`landing`] classifier. The storage lives behind the
//! [`crate::application::ports::SandboxProfileRepository`] port. Nothing here reads
//! `$HOME`, shells out, or touches Docker.
//!
//! ## The four load-bearing ideas
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
//!    ([`crate::adapters::staging::prepare`]) and the mount view
//!    ([`crate::adapters::staging::extra_mounts`]). They therefore cannot drift, and the
//!    "an entry under `.claude/` produces no extra `-v`" dedup of ADR-0031 §4 is not a
//!    special case: it is a *consequence*.
//!
//! 3. **The env is a MAP, not a diff** (#468, ADR-0031 §8). Every other field here is an
//!    intention folded over a built-in default, because the default evolves with the
//!    release. `env` has no built-in default and never will — PDO poses the run-constant
//!    variables itself, at `docker create`, and the three it owns are *refused* as profile
//!    keys ([`crate::domain::container::RUN_CONSTANT_ENV_KEYS`]). So there is nothing to
//!    diff against: the stored map IS the effective map, and [`ResolvedProfile::env`] is a
//!    copy rather than a resolution. A `disabled`-style negative list would be answering a
//!    question nobody asked.
//!
//! 3b. **The image source is a profile field too** (#467, ADR-0031 §9). Same shape as `env` —
//!    not a diff, `None` meaning "this profile poses nothing" — but with one extra property: it is
//!    the only field whose two shapes ([`crate::domain::image::ProfileImage`]) are mutually
//!    exclusive rather than additive, so it is a **tagged enum in one column**, not two nullable
//!    columns that could disagree. Unlike `env` it DOES have a built-in default since #471
//!    ([`DEFAULT_PROFILE_IMAGE`]): the two instance-wide settings it used to fall back to are gone,
//!    and what they resolved to by default became a constant of this layer, next to
//!    [`DEFAULT_FULL_ENTRIES`]. It is still not a *diff* — an image is one value, not a list, so
//!    "poses nothing" resolves to the default outright rather than folding against it.
//!
//! 4. **A harness's staging set is not editable, and not an entry either** (ADR-0031
//!    §1, ADR-0063). The five guarantees the staging holds for `claude` in every
//!    profile are satisfied either by an entry or by a fallback synthesis. Two of them
//!    need *keys* in a file the profile may also carry (class **(b)**) — unchecking
//!    those is safe. Three need the *whole* file, so they are class **(c)**: shown
//!    read-only, never checkable, and **refused as extras** too — for the entries and
//!    transcript sinks of **every** declared set ([`set_owned_paths`]).
//!
//! ## No rename in v1
//!
//! `name` is the primary key AND the value stored by all three consumers
//! (`triggers.sandbox`, `instance_config.default_sandbox`, the frozen `RunStarted`
//! payload). Renaming is therefore *delete + create*. A future rename must be a
//! **repointing transaction** across those three stores — never an `UPDATE` of this key.

pub mod defaults;
pub mod draft;
pub mod validate;

use std::collections::BTreeMap;

use serde::Serialize;

pub use defaults::{
    base_entries, set_owned_paths, DefaultEntry, DefaultProfileImage, EntryKind, StagingGuarantee,
    DEFAULT_FULL_ENTRIES, DEFAULT_PROFILE_IMAGE, STAGING_GUARANTEES,
};

use crate::domain::image::ProfileImage;

/// The virtual default that carries the full replica of the host `~/.claude`.
pub const FULL_PROFILE: &str = "full";
/// The virtual default that carries **nothing** in its own right: `minimal` *is* the
/// harness staging set alone, which is exactly the empty entry list.
pub const MINIMAL_PROFILE: &str = "minimal";

/// The two names that resolve with no database row (ADR-0031 §2). Creating one *is*
/// materialising it, which is why [`validate::validate_profile_name`] allows them.
pub const VIRTUAL_PROFILES: &[&str] = &[FULL_PROFILE, MINIMAL_PROFILE];

/// `$HOME`-relative prefixes we stage happily but flag in the UI (ADR-0031 §3).
/// Forbidding them would be theatre while ADR-0030 already assumes the host uid, the
/// repo mounted rw and real Claude credentials.
pub const SENSITIVE_PREFIXES: &[&str] = &[".ssh", ".aws", ".gnupg"];

/// Where an entry lands in the staging. The SINGLE classifier shared by the copy view
/// and the mount view (see the module header, idea 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Landing<'a> {
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
pub fn landing(entry: &str) -> Landing<'_> {
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

/// Outcome of folding a stored diff over a base list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedEntries {
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
pub fn resolve_entry_list(
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

/// The **names** of a profile's env, comma-joined — the ONLY rendering of an env map that
/// may reach a log (#468).
///
/// It exists as a named function rather than an inline `format!` precisely so the rule is
/// testable: `ensure_ready` already logs the staging entry list in clear, and copying that
/// shape for the env would put a client's API token in the systemd journal, which outlives
/// the Run. A unit test asserts no value survives this call.
pub fn env_names(env: &BTreeMap<String, String>) -> String {
    env.keys().cloned().collect::<Vec<_>>().join(", ")
}

/// A materialised profile row: the user's **intention**, never the effective list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProfileDiff {
    pub name: String,
    pub disabled: Vec<String>,
    pub extras: Vec<String>,
    /// Environment variables posed at `docker create` (#468, ADR-0031 §8). Unlike the two
    /// fields above this is NOT a diff — see idea 3 in the module header. `BTreeMap` so a
    /// key can carry exactly one value and the argv order is deterministic without the
    /// caller sorting.
    pub env: BTreeMap<String, String>,
    /// Where this profile's container image comes from (#467, ADR-0031 §9). `None` = the
    /// profile poses nothing and [`DEFAULT_PROFILE_IMAGE`] decides (#471), which is what every
    /// pre-#467 profile keeps doing.
    pub image: Option<ProfileImage>,
    pub updated_at: String,
}

/// A profile as the API serves it: the stored diff **plus** what it resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProfile {
    pub name: String,
    /// One of [`VIRTUAL_PROFILES`] — i.e. it resolves with or without a row.
    pub is_virtual: bool,
    /// A row exists (the profile has been edited at least once).
    pub materialised: bool,
    pub disabled: Vec<String>,
    pub extras: Vec<String>,
    pub resolved: ResolvedEntries,
    /// The profile's env (#468). Empty for a virtual default with no row — there is no
    /// built-in env to fall back to, by design (module header, idea 3).
    pub env: BTreeMap<String, String>,
    /// The profile's image source (#467). `None` for a virtual default with no row, and for
    /// every profile that poses nothing — the two are indistinguishable on purpose: both mean
    /// "[`DEFAULT_PROFILE_IMAGE`] decides".
    pub image: Option<ProfileImage>,
    pub updated_at: Option<String>,
}

/// Resolve `name` from its stored diff (if any) to its effective entry list, or `None`
/// when no such profile exists.
///
/// This is the ONE existence rule: `None` is what every edge turns into a hard
/// failure (400 at create, a red Trigger fire record, `RunFailed` at boot recovery) so
/// an unknown name can never fall back silently to less isolation (ADR-0031 §7).
pub fn resolve_profile(name: &str, stored: Option<ProfileDiff>) -> Option<ResolvedProfile> {
    let is_virtual = VIRTUAL_PROFILES.contains(&name);
    let Some(diff) = stored else {
        if !is_virtual {
            return None;
        }
        let base = base_entries(name);
        return Some(ResolvedProfile {
            name: name.to_string(),
            is_virtual,
            materialised: false,
            disabled: Vec::new(),
            extras: Vec::new(),
            resolved: resolve_entry_list(&base, &[], &[]),
            env: BTreeMap::new(),
            image: None,
            updated_at: None,
        });
    };
    // The stored row's own `name` casing wins — it is what the PK holds.
    let base = base_entries(&diff.name);
    let resolved = resolve_entry_list(&base, &diff.disabled, &diff.extras);
    Some(ResolvedProfile {
        name: diff.name,
        is_virtual,
        materialised: true,
        disabled: diff.disabled,
        extras: diff.extras,
        resolved,
        env: diff.env,
        image: diff.image,
        updated_at: Some(diff.updated_at),
    })
}

/// Every profile name the instance can serve: the two virtual defaults ∪ the
/// materialised rows, deduplicated (an edited `full` appears once), sorted, each with
/// whether it is virtual.
pub fn profile_names(materialised: impl IntoIterator<Item = String>) -> Vec<(String, bool)> {
    let mut names: Vec<String> = VIRTUAL_PROFILES.iter().map(|s| s.to_string()).collect();
    for name in materialised {
        if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    names.sort();
    names
        .into_iter()
        .map(|n| {
            let v = VIRTUAL_PROFILES.contains(&n.as_str());
            (n, v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn diff(name: &str) -> ProfileDiff {
        ProfileDiff {
            name: name.into(),
            disabled: v(&[".claude/plugins"]),
            extras: v(&[".gitconfig"]),
            env: BTreeMap::from([("K".to_string(), "v".to_string())]),
            image: None,
            updated_at: "2026-01-01T00:00:00.000Z".into(),
        }
    }

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

    /// `minimal` IS the staging set alone: the empty list, not an error and not a special case.
    #[test]
    fn minimal_default_resolves_to_the_empty_list() {
        let base = base_entries(MINIMAL_PROFILE);
        assert!(base.is_empty());
        assert!(resolve_entry_list(&base, &[], &[]).entries.is_empty());
    }

    /// GOLDEN, load-bearing: `.claude.json` is the SIBLING, never under `claude-home/`.
    /// Filed there it would surface at `$HOME/.claude/.claude.json`, where Claude Code
    /// never looks — see the note on
    /// [`crate::domain::staging::staged_claude_json`]. Do not "tidy" this.
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

    /// AC4 of #468: what may reach a log is the NAMES. This is the whole guarantee, so it
    /// is pinned on the function that produces the log text rather than on a log capture.
    #[test]
    fn env_names_never_leaks_a_value() {
        let env: BTreeMap<String, String> = [
            ("TOKEN", "sk-ant-super-secret"),
            ("PUPPETEER_EXECUTABLE_PATH", "/usr/bin/chromium"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let rendered = env_names(&env);
        assert_eq!(rendered, "PUPPETEER_EXECUTABLE_PATH, TOKEN");
        for value in env.values() {
            assert!(
                !rendered.contains(value),
                "a value leaked into the log rendering: {rendered}"
            );
        }
        assert_eq!(env_names(&BTreeMap::new()), "");
    }

    #[test]
    fn a_virtual_default_resolves_without_a_row_and_an_unknown_name_does_not() {
        let full = resolve_profile(FULL_PROFILE, None).expect("full is virtual");
        assert!(full.is_virtual && !full.materialised);
        assert_eq!(full.resolved.entries.len(), DEFAULT_FULL_ENTRIES.len());
        assert!(full.env.is_empty() && full.image.is_none() && full.updated_at.is_none());
        assert_eq!(resolve_profile("team", None), None);
    }

    #[test]
    fn a_stored_diff_folds_over_its_base() {
        let team = resolve_profile("team", Some(diff("team"))).unwrap();
        assert!(!team.is_virtual && team.materialised);
        assert!(team.resolved.entries.contains(&".gitconfig".to_string()));
        assert!(!team
            .resolved
            .entries
            .contains(&".claude/plugins".to_string()));
        assert_eq!(team.env["K"], "v");
        let edited_minimal = resolve_profile(MINIMAL_PROFILE, Some(diff(MINIMAL_PROFILE))).unwrap();
        assert!(edited_minimal.is_virtual && edited_minimal.materialised);
        assert_eq!(edited_minimal.resolved.entries, v(&[".gitconfig"]));
    }

    #[test]
    fn profile_names_are_the_virtual_defaults_and_the_rows_once_each() {
        assert_eq!(
            profile_names(v(&["team", "full", "Minimal"])),
            vec![
                ("full".to_string(), true),
                ("minimal".to_string(), true),
                ("team".to_string(), false),
            ]
        );
    }
}
