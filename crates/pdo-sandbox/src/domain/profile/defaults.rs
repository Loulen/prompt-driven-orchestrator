//! The built-in defaults every staging profile diffs against (ADR-0031 §2): the
//! `full` entry list, the default image, and the staging-set guarantees shown
//! read-only in the editor.

use serde::Serialize;
use ts_rs::TS;

use super::MINIMAL_PROFILE;
use crate::domain::image::ImageSource;

/// What an entry points at, for the editor's right-hand column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SandboxEntryKind")]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    Dir,
    File,
    /// A one-level filename pattern (only `.claude/*.md` ships as one). Users cannot
    /// author these — [`super::validate::validate_entry`] rejects `*` — so the pattern
    /// in a resolved list can only have come from the default.
    Glob,
}

/// One entry of the built-in default (the `full` profile's 9 lines, ADR-0031 §2).
pub struct DefaultEntry {
    /// `$HOME`-relative path or one-level glob.
    pub path: &'static str,
    pub kind: EntryKind,
    /// Class **(b)**: unchecking it does NOT make the file absent — an autonomy fixup
    /// of the staging set re-synthesises the keys it needs. Exactly two entries, and the UI must say so,
    /// or unchecking looks more destructive than it is.
    pub resynthesised: bool,
    /// Static, server-owned advisory shown under the entry. Static on purpose: a real
    /// recursive size walk of `plugins/**/node_modules` is seconds of IO, and the
    /// settings handler explicitly budgets against that (#373 discipline — the daemon
    /// owns every derived label, the client computes none).
    pub note: Option<&'static str>,
}

/// The `full` profile — the built-in default every profile except `minimal` diffs
/// against. Order here is the *editor's* reading order; [`super::resolve_entry_list`] sorts.
///
/// `.credentials.json` is deliberately absent: it is a **staging set** entry (G1), and
/// having it in both places made the constant lie about being a list of entries.
pub const DEFAULT_FULL_ENTRIES: &[DefaultEntry] = &[
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

/// L'image qu'un profil **qui n'en pose pas** résout (#471, ADR-0031 §9) — le pendant image de
/// [`DEFAULT_FULL_ENTRIES`], et pour la même raison : c'est un **défaut de profil**, plus un
/// réglage d'instance.
///
/// Pourquoi une `struct` et non deux constantes nues : les deux champs ne se lisent que
/// *ensemble* (un `ImageSource::Registry` sur un Dockerfile qui n'est pas celui publié en amont
/// ne pulle rien), et le jour où le défaut change il doit changer d'un bloc.
///
/// Défini ici — pas dans [`crate::domain::image`], qui possède le *vocabulaire* d'image
/// ([`crate::domain::image::ProfileImage`], le hash, les refs) — par symétrie exacte avec
/// `ProfileImage` : là-bas le vocabulaire appartient à l'image et le profil le stocke, ici le
/// défaut appartient à la couche de défauts de profil et l'image le lit. Un fait, un
/// propriétaire (discipline #447).
pub struct DefaultProfileImage {
    /// Pull-ou-build de l'image hash-dérivée. `Registry` = ce que le tier `default` de
    /// l'ex-réglage `image_source` valait, donc un profil qui ne pose rien pulle encore.
    pub source: ImageSource,
    /// `None` = **l'emplacement seedé** `<sandbox_root>/Dockerfile`, résolu par
    /// [`crate::domain::image::default_dockerfile_path`]. Un chemin littéral ici mentirait : le
    /// défaut dépend de `<sandbox_root>`, donc de `$HOME`, que cette couche pure ne lit pas.
    /// `Some(path)` n'existe que pour rendre le champ *exprimable* — rien ne le pose aujourd'hui.
    pub dockerfile: Option<&'static str>,
}

/// Le défaut, en un bloc. Un profil qui ne pose pas d'image produit ce que l'instance produisait
/// avant #471, bit pour bit — pinné par le golden de l'image sur le ref calculé (AC 3).
pub const DEFAULT_PROFILE_IMAGE: DefaultProfileImage = DefaultProfileImage {
    source: ImageSource::Registry,
    dockerfile: None,
};

/// One class-**(c)** staging-set guarantee: satisfied by the *whole* file, so it is
/// neither checkable nor addable. Shown read-only in the editor because without that
/// block a `minimal` profile's screen looks broken and the user wrongly concludes the
/// container starts with no credentials.
pub struct StagingGuarantee {
    pub id: &'static str,
    /// What the container gets, in the user's words.
    pub label: &'static str,
    /// The `$HOME`-relative path the staging set owns, when there is one.
    pub path: Option<&'static str>,
}

/// `claude`'s staging set in the user's words, verbatim from
/// [`pdo_harness::probes::CLAUDE_STAGING_SET`] (ADR-0031 §1 G1–G5). G3/G4 appear
/// here as guarantees even though their files are class-(b) *entries* — the
/// difference is the rule that decides the classes: **(b) = the fixup needs keys in
/// the file; (c) = the set needs the file whole.**
///
/// Served on the wire under the historical key `floor` (the editor's read-only block):
/// a wire field, frozen for the frontend and its tests, not a term this code uses.
pub const STAGING_GUARANTEES: &[StagingGuarantee] = &[
    StagingGuarantee {
        id: "credentials",
        label: "Valid Claude credentials",
        path: Some(".claude/.credentials.json"),
    },
    StagingGuarantee {
        id: "org-managed-settings",
        label: "Your organisation's managed settings, consented",
        path: Some(".claude/remote-settings.json"),
    },
    StagingGuarantee {
        id: "bypass-permissions",
        label: "Permissions bypass accepted (no blocking dialog)",
        path: None,
    },
    StagingGuarantee {
        id: "run-root-trust",
        label: "Trust pre-granted on the Run's repo root",
        path: None,
    },
    StagingGuarantee {
        id: "empty-projects",
        label: "An empty projects/ transcript sink",
        path: Some(".claude/projects"),
    },
];

/// Paths a staging set owns **whole** — refused as extras, never checkable: the
/// entries and the transcript sinks of **every** declared set (ADR-0063), read from
/// the dispatch table so a harness that declares a set is protected with no second
/// edit. Copying a transcript sink would break the merge-back idempotence and the
/// cost fold; copying a set entry would make the profile lie about owning it.
pub fn set_owned_paths() -> Vec<&'static str> {
    pdo_harness::probes::staging_sets()
        .into_iter()
        .flat_map(|(_, set)| {
            set.entries
                .iter()
                .map(|e| e.rel)
                .chain(set.transcripts.iter().copied())
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The built-in default a profile diffs against: **empty** for `minimal` (which *is*
/// the staging set alone), the `full` list for every other name.
///
/// So a brand-new profile starts as a copy of `full` with everything checked — which is
/// what makes "uncheck `.claude/plugins`" the two-click operation the issue asks for.
pub fn base_entries(name: &str) -> Vec<&'static str> {
    if name == MINIMAL_PROFILE {
        Vec::new()
    } else {
        DEFAULT_FULL_ENTRIES.iter().map(|e| e.path).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::profile::FULL_PROFILE;

    /// Exactly two class-(b) entries — the set's fixups re-synthesise those two files, so
    /// unchecking them is safe. The UI copy depends on this count being right.
    #[test]
    fn exactly_two_default_entries_are_resynthesised_by_the_fixups() {
        let resynth: Vec<&str> = DEFAULT_FULL_ENTRIES
            .iter()
            .filter(|e| e.resynthesised)
            .map(|e| e.path)
            .collect();
        assert_eq!(resynth, vec![".claude.json", ".claude/settings.json"]);
    }

    /// `minimal` IS the staging set alone: the empty list, not an error and not a special case.
    #[test]
    fn minimal_diffs_against_nothing_and_every_other_name_against_full() {
        assert!(base_entries(MINIMAL_PROFILE).is_empty());
        assert_eq!(base_entries(FULL_PROFILE).len(), DEFAULT_FULL_ENTRIES.len());
        assert_eq!(base_entries("team").len(), DEFAULT_FULL_ENTRIES.len());
    }
}
