//! The sandbox staging set a harness declares (ADR-0063).

use super::probes_for;

/// One `$HOME`-relative entry of a harness's staging set (ADR-0063 §1): copied
/// from the host into the Run's staging on the way in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagingEntry {
    /// `$HOME`-relative path (`.claude/.credentials.json`, `.pi/agent/auth.json`).
    /// Routed by the same classifier as a profile entry
    /// (`sandbox_profile::landing`), so the copy view and the mount view
    /// cannot drift.
    pub rel: &'static str,
    /// What to log at `info` when the host lacks the entry, or `None` for a silent
    /// skip. `Some` is for an entry whose absence is the **common** case yet worth a
    /// line (an org baseline on an install with no org): a `warn!` per Run would
    /// train the reader to ignore warnings.
    pub absent_note: Option<&'static str>,
}

/// A write that disarms a **blocking dialog** once a staging set is copied
/// (ADR-0063 §2). Distinct from the set itself: the set is what the harness
/// *reads* to behave as on the host; a fixup is what PDO *writes* so an unattended
/// session never waits for a human. `claude` has them; `copilot` and `pi` carry the
/// equivalent on their argv (`--allow-all`, `-a`) and declare none.
///
/// Each variant is applied by `sandbox_staging`, the only module that
/// writes into a staging — this enum only *names* the write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutonomyFixup {
    /// `skipDangerousModePermissionPrompt: true` in the staged `.claude/settings.json`
    /// — merged into the host copy when the profile staged it, synthesised as a
    /// one-key file otherwise (ADR-0031 §1 G3).
    ClaudeBypassPermissions,
    /// The staged `.claude.json` baseline: `hasCompletedOnboarding`, plus trust
    /// pre-granted on the Run root when there is one (ADR-0031 §1 G4).
    ClaudeJsonBaseline,
}

/// The sandbox **staging set** a harness declares (ADR-0063 §1): the `$HOME`
/// entries and env that make a session in the container behave as on the host,
/// plus its **transcript sinks** and its [`AutonomyFixup`]s.
///
/// Absent ⇒ a sandboxed Run on this harness holds only by the user's image and
/// the profile's `$HOME` exceptions — and PDO says so once (see
/// [`staging_set_absence_note`]). `None` is a declared value (ADR-0051), published
/// in the support table.
///
/// The set is **data**; `sandbox_staging` is the one interpreter. It is
/// copied at the spawn of the **first** session of the Run that resolves the harness
/// (`sandbox_staging::fill_staging_set`, #708, ADR-0063 §3) — never at `RunStarted`,
/// never for a harness the Run does not launch. Its `home_root` is mounted **empty**
/// at container creation so that later copy lands under a mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StagingSet {
    /// The support-table cell: what this harness stages, in the reader's words.
    pub label: &'static str,
    /// The harness's home root under `$HOME` (`.claude`, `.pi/agent`): the directory
    /// mounted (empty until filled) for every first-party harness (ADR-0063 §3).
    pub home_root: &'static str,
    /// Entries copied from the host, whole. Refused as profile entries: the set owns
    /// them in every profile.
    pub entries: &'static [StagingEntry],
    /// `$HOME`-relative sub-paths **skipped** when an entry above is a directory.
    pub excludes: &'static [&'static str],
    /// Env vars the harness needs inside the container (`PI_TELEMETRY=0`). Posed
    /// with the set; `claude` needs none.
    pub env: &'static [(&'static str, &'static str)],
    /// `$HOME`-relative transcript sinks: created **empty** on the way in (never
    /// copied — copying would break the merge-back idempotence and the cost fold),
    /// **harvested** at merge-back under the same encoded dirname. Refused as
    /// profile entries.
    pub transcripts: &'static [&'static str],
    /// The blocking-dialog disarms applied once the set is on disk.
    pub fixups: &'static [AutonomyFixup],
}

impl StagingSet {
    pub fn label(self) -> &'static str {
        self.label
    }
}

/// `claude`'s staging set — the five guarantees of ADR-0031 §1, byte for byte,
/// re-expressed as data (ADR-0063 amends §1: they are *claude's*, one set among
/// others):
///
/// - **G1 credentials** → entry `.claude/.credentials.json` (0600 preserved by
///   `std::fs::copy`; absent on the host → silent no-op, auth fails later and it is
///   not `prepare`'s call);
/// - **G2 org managed settings** → entry `.claude/remote-settings.json` (absent →
///   `info!` no-op, the majority case; present but uncopyable → hard error, a
///   compliance surprise);
/// - **G3 bypass permissions** → [`AutonomyFixup::ClaudeBypassPermissions`];
/// - **G4 `.claude.json` baseline** → [`AutonomyFixup::ClaudeJsonBaseline`];
/// - **G5 empty transcript sink** → transcripts `.claude/projects`.
///
/// The `~/.claude` allowlist itself (skills, plugins, settings…) is NOT here: it is
/// the user's `full` profile (`sandbox_profile::DEFAULT_FULL_ENTRIES`),
/// the harness-agnostic diff of ADR-0063 §4.
pub const CLAUDE_STAGING_SET: StagingSet = StagingSet {
    label: "the `.claude` home — credentials and org managed settings copied, trust and \
            permissions bypass fixed up, transcripts harvested back",
    home_root: ".claude",
    entries: &[
        StagingEntry {
            rel: ".claude/.credentials.json",
            absent_note: None,
        },
        StagingEntry {
            rel: ".claude/remote-settings.json",
            absent_note: Some("nothing to consent to (no-op)"),
        },
    ],
    excludes: &[],
    env: &[],
    transcripts: &[".claude/projects"],
    fixups: &[
        AutonomyFixup::ClaudeBypassPermissions,
        AutonomyFixup::ClaudeJsonBaseline,
    ],
};

/// `pi`'s staging set (#708, ADR-0063): the whole `.pi/agent/` home **except**
/// `sessions/` — `auth.json`, `settings.json`, the model catalogue (`models.json`),
/// extensions, skills, prompts, themes and `bin/` — so a session in the container
/// authenticates, prices its messages and finds its extensions exactly as on the
/// host. Two env vars ride the `docker exec` with it: `PI_SKIP_VERSION_CHECK=1` (PDO
/// decides when the target moves, not a startup banner) and `PI_TELEMETRY=0`. **No**
/// `PI_OFFLINE` — the catalogue is copied, the network is not cut.
///
/// `sessions/` is the transcript sink: created empty on the way in and harvested at
/// merge-back under the same cwd-encoded dirname (the worktree is mounted at its
/// host path, ADR-0030, so pi names the folder identically inside and out). **No**
/// autonomy fixup: `-a` on pi's argv already disarms every approval dialog.
pub const PI_STAGING_SET: StagingSet = StagingSet {
    label: "the `.pi/agent` home — auth, settings, model catalogue, extensions, skills, \
            prompts, themes and bin copied, sessions harvested back",
    home_root: ".pi/agent",
    entries: &[StagingEntry {
        rel: ".pi/agent",
        absent_note: Some(
            "pi is not set up on this host; the container starts with an empty pi home",
        ),
    }],
    excludes: &[],
    env: &[("PI_SKIP_VERSION_CHECK", "1"), ("PI_TELEMETRY", "0")],
    transcripts: &[".pi/agent/sessions"],
    fixups: &[],
};

/// Every staging set a first-party harness declares, in registry order. The
/// generic consumers (the transcript merge-back, the profile grammar's refusals)
/// iterate this rather than naming `claude`, so a harness that declares a set is
/// harvested and protected with no second edit (ADR-0063).
pub fn staging_sets() -> Vec<(String, StagingSet)> {
    crate::domain::registry::embedded_floor()
        .into_iter()
        .filter_map(|d| {
            let set = probes_for(&d.name)?.staging_set()?;
            Some((d.name, set))
        })
        .collect()
}
