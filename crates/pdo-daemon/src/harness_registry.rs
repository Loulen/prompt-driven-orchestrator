//! Resolve an agentic-harness name to its descriptor (ADR-0045).
//!
//! A **harness** is the program that runs a NodeRun's agent (`claude`,
//! `opencode`, …). It declares itself with two argv-token templates (launch,
//! resume), an env block, and the binary PDO probes at spawn — never with named
//! per-feature fields (a named field per case becomes a spelling per case; the
//! template covers them without naming, ADR-0045).
//!
//! **This slice is the embedded floor only.** `claude` and `opencode` are compiled
//! in; nothing is seeded on disk and this module never reads `$HOME` (the
//! discipline `run_cost` paid for in #408 — a root goes in, a descriptor comes
//! out). A user-declared *disk tier* that merges over the floor **by name** is
//! #553; [`merge_by_name`] is that seam, present and tested now so the disk tier
//! layers on without rewriting [`resolve`]'s callers — but no caller passes a disk
//! tier in this slice, so the floor is the whole registry.
//!
//! **#553 lands that disk tier.** [`HarnessRegistry::load`] reads a user-declared
//! descriptor file under an **injected root** (never `$HOME` — the discipline this
//! module already keeps), parses it, and merges it over the embedded floor **by
//! name** ([`merge_by_name`]). Nothing is ever written or seeded. A descriptor that
//! is unreadable or refused is **inert and diagnosed**: its key falls through to
//! the next tier (the floor), it is never partially applied, and it is named —
//! once per distinct diagnostic in the log, and always in `GET /settings` — the
//! exact idiom of `price_table` (ADR-0034).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Deserialize;
use tracing::warn;

/// A harness, as PDO launches / resumes / attaches it (ADR-0045).
///
/// The two templates are rendered by [`crate::harness_argv`]; the caller fills
/// the holes (`{prompt}`, `{model}`, `{effort}`, `{session_id}`, `{settings}`,
/// and the resume-only `{resume}` selector). The env block is exported before an
/// **agent** tail — that is where `claude`'s `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1`
/// now comes from (AC #4); `script` / `shell` tails get it forced by the wrapper.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessDescriptor {
    /// The harness name (`claude`, `opencode`) — the registry key.
    pub name: String,
    /// The program that runs the agent — probed on `PATH` at spawn. Not found ⇒
    /// the spawn fails **fast**, naming the harness, and writes no start event
    /// (ADR-0037).
    pub binary: String,
    /// The launch tail as an argv-token template.
    pub launch: Vec<String>,
    /// The resume tail as an argv-token template. **Empty** ⇒ the harness has no
    /// resume mechanism, so a resume serves the last pane snapshot rather than an
    /// error (AC #9, same branch as a `script` node).
    pub resume: Vec<String>,
    /// Env exported before an agent tail (`K=V`). Rendered by the wrapper.
    pub env: Vec<(String, String)>,
}

impl HarnessDescriptor {
    /// Whether the LAUNCH template carries an `{effort}` hole.
    ///
    /// `opencode` has no effort axis at launch (measured on 1.18.18: the effort
    /// variant is an in-session command, not a flag), so the UI greys its effort
    /// picker off this fact alone — an absence declared by the descriptor's shape,
    /// no extra flag needed (AC #13, ADR-0045).
    pub fn has_effort_hole(&self) -> bool {
        self.launch.iter().any(|t| t.contains("{effort}"))
    }

    /// Whether the harness can re-enter a saved conversation. `false` ⇒ a resume
    /// serves the pane snapshot (AC #9).
    pub fn can_resume(&self) -> bool {
        !self.resume.is_empty()
    }

    /// Whether the LAUNCH template pins a session identity (`{session_id}`).
    ///
    /// `opencode` cannot (measured: launching with a fresh id answers "Session not
    /// found" and exits 1 — its selector *continues* a session, never creates
    /// one), so PDO never pins one for it and attributes by working dir alone.
    pub fn pins_session_id(&self) -> bool {
        self.launch.iter().any(|t| t.contains("{session_id}"))
    }
}

/// The `claude` harness name — the floor of the precedence chain (ADR-0046).
pub const CLAUDE: &str = "claude";
/// The `opencode` harness name (ADR-0045, measured on 1.18.18).
pub const OPENCODE: &str = "opencode";

/// The `claude` descriptor: the legacy launch, expressed as data.
///
/// The launch template reproduces the pre-#550 `build_agent_tail` **byte for
/// byte** once its holes are empty — that is the #550 gate, pinned by the goldens
/// in [`crate::harness_argv`]. The resume template reproduces the pre-#550
/// `build_resume_script` tail; its `{resume}` hole is the identity-or-blind
/// selector the resume seam computes (ADR-0045 keeps that choice in code).
pub fn claude() -> HarnessDescriptor {
    HarnessDescriptor {
        name: CLAUDE.to_string(),
        binary: "claude".to_string(),
        launch: [
            "exec",
            "claude",
            "--dangerously-skip-permissions",
            "--model {model}",
            "--effort {effort}",
            "--settings {settings}",
            "--session-id {session_id}",
            "{prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: [
            "exec",
            "claude",
            "--dangerously-skip-permissions",
            "{resume}",
            "--effort {effort}",
            "--settings {settings}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        // AC #4: the CCR suppression now comes from the descriptor for an agent
        // tail (byte-identical to the wrapper's old hard-coded export — see the
        // `harness_env` handling in `tmux_session_manager::wrap_with_env`).
        env: vec![(
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC".to_string(),
            "1".to_string(),
        )],
    }
}

/// The `opencode` descriptor (measured on 1.18.18).
///
/// `--auto` is its `--dangerously-skip-permissions`; the prompt is a `--prompt`
/// argument; the model is `--model provider/model`. There is **no** effort hole
/// (no launch-time effort axis) and **no** session-id hole (identity can't be
/// pinned), so the effort picker greys and attribution falls back to the working
/// dir. Resume is a blind `--continue` (opencode is resident and continues the
/// cwd's latest session); it carries no env block.
pub fn opencode() -> HarnessDescriptor {
    HarnessDescriptor {
        name: OPENCODE.to_string(),
        binary: "opencode".to_string(),
        launch: [
            "exec",
            "opencode",
            "--auto",
            "--model {model}",
            "--prompt {prompt}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        resume: ["exec", "opencode", "--auto", "--continue"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
        env: vec![],
    }
}

/// The embedded floor: the harnesses PDO ships compiled in, in precedence-neutral
/// declaration order.
pub fn embedded_floor() -> Vec<HarnessDescriptor> {
    vec![claude(), opencode()]
}

/// Merge a user-declared disk tier over the embedded floor, **by name**: a disk
/// descriptor replaces the floor's entry of the same name; a floor name absent
/// from disk survives. Pure — the caller (a future slice, #553) reads and parses
/// the disk tier and hands the descriptors in. No caller passes a disk tier in
/// this slice, so `merge_by_name(embedded_floor(), vec![])` is the whole registry.
pub fn merge_by_name(
    floor: Vec<HarnessDescriptor>,
    disk: Vec<HarnessDescriptor>,
) -> Vec<HarnessDescriptor> {
    let mut merged = floor;
    for d in disk {
        match merged.iter_mut().find(|f| f.name == d.name) {
            Some(slot) => *slot = d,
            None => merged.push(d),
        }
    }
    merged
}

/// Resolve a harness name to its descriptor. `None` ⇒ no harness carries that
/// name (an unknown harness — the spawn seam turns this into a fail-fast that
/// names it, never a silent fallback).
///
/// The disk tier (#553) layers on by making this `merge_by_name(embedded_floor(),
/// disk).into_iter().find(...)`; callers don't change.
pub fn resolve(name: &str) -> Option<HarnessDescriptor> {
    embedded_floor().into_iter().find(|d| d.name == name)
}

// --- The disk tier (#553) -----------------------------------------------------

/// A refused disk descriptor and why — the material of BOTH the `warn!` and the
/// `GET /settings` `reason`, so the two can never drift (idiom of
/// `price_table::RejectedRow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedDescriptor {
    pub name: String,
    pub why: String,
}

/// The result of parsing the on-disk descriptor tier. Pure: text in; the parsed
/// descriptors and any refusals out. An unparseable document yields
/// `unparseable: Some(err)` and NO descriptors — never an `Err`, because a bad
/// descriptor file must not be able to fail a spawn (it degrades to the floor).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedDescriptors {
    pub descriptors: Vec<HarnessDescriptor>,
    pub rejected: Vec<RejectedDescriptor>,
    pub unparseable: Option<String>,
}

/// One row of the descriptor file. The harness **name is the map key**, so it is
/// not a field here (key uniqueness is structural, like `price_table`'s manual
/// map). No `deny_unknown_fields` (ADR-0015 #471: an unknown field is ignored).
#[derive(Debug, Deserialize)]
struct DescriptorRow {
    binary: Option<String>,
    #[serde(default)]
    launch: Vec<String>,
    #[serde(default)]
    resume: Vec<String>,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Parse the descriptor file (`descriptors.yaml`). PURE.
///
/// Shape — a MAP keyed by harness name, mirroring `price_table`'s `models.yaml`:
///
/// ```yaml
/// harnesses:
///   my-harness:
///     binary: my-harness
///     launch: ["exec", "my-harness", "--auto", "--prompt {prompt}"]
///     resume: ["exec", "my-harness", "--auto", "--continue"]   # optional
///     env: { FOO: bar }                                        # optional
/// ```
///
/// A row is **refused whole** (never partially applied) when it lacks the two
/// things PDO needs to *launch* it: a `binary` to probe and a non-empty `launch`
/// argv template (ADR-0045). A refused row's key falls through to the next tier —
/// so a broken `claude:` override leaves the embedded `claude` untouched. PDO does
/// **not** validate what the argv *means* (ADR-0001): a descriptor without an
/// autonomy flag simply yields a node stalled on a permission dialog, which is
/// said in the descriptor's documentation, not guarded here.
pub fn parse_descriptors(text: &str) -> ParsedDescriptors {
    #[derive(Deserialize)]
    struct Doc {
        #[serde(default)]
        harnesses: BTreeMap<String, DescriptorRow>,
    }

    // A blank/comment-only file is an empty tier, not a broken one — the normal
    // state of an instance that has the dir but no descriptor yet.
    if text.trim().is_empty() {
        return ParsedDescriptors::default();
    }
    let doc: Doc = match serde_yaml::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return ParsedDescriptors {
                unparseable: Some(e.to_string()),
                ..Default::default()
            }
        }
    };

    let mut parsed = ParsedDescriptors::default();
    for (name, row) in doc.harnesses {
        if name.trim().is_empty() {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: "a harness name must not be blank".to_string(),
            });
            continue;
        }
        let binary = row.binary.unwrap_or_default();
        if binary.trim().is_empty() {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: "missing `binary` — PDO probes a program at spawn; a descriptor with none \
                      could only ever fail-fast"
                    .to_string(),
            });
            continue;
        }
        // An empty string among the launch tokens is not a hole to fill — it would
        // render to a stray token. Refuse the row rather than launch something odd.
        if row.launch.is_empty() {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: "missing `launch` — a harness declares itself by an argv template (ADR-0045)"
                    .to_string(),
            });
            continue;
        }
        parsed.descriptors.push(HarnessDescriptor {
            name,
            binary,
            launch: row.launch,
            resume: row.resume,
            env: row.env.into_iter().collect(),
        });
    }
    parsed
}

/// The registry resolved for one read: the embedded floor merged with the disk
/// tier **by name**, plus the diagnostics for whatever the disk tier refused. The
/// analogue of `price_table::PriceTable` — one resolver shared by the spawn/resume
/// seams and the `GET /settings` view, so what the UI lists can never drift from
/// what actually resolves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessRegistry {
    /// Floor ∪ disk, merged by name (a disk descriptor replaces the floor entry of
    /// its name; a floor name absent from disk survives).
    descriptors: Vec<HarnessDescriptor>,
    disk_rejected: Vec<RejectedDescriptor>,
    disk_unparseable: Option<String>,
    /// Set by [`Self::load`] only, so [`Self::diagnostic`] can name the real file.
    disk_path: Option<PathBuf>,
}

impl Default for HarnessRegistry {
    fn default() -> Self {
        Self::builtin()
    }
}

impl HarnessRegistry {
    /// The floor alone — no IO, no `$HOME`. What every non-root caller (infra
    /// sessions, the golden tests) resolves against.
    pub fn builtin() -> Self {
        Self {
            descriptors: embedded_floor(),
            disk_rejected: Vec::new(),
            disk_unparseable: None,
            disk_path: None,
        }
    }

    /// `<home_root>/.pdo/harnesses/descriptors.yaml` — path arithmetic only, the
    /// idiom of `price_table::PriceTable::paths`.
    pub fn descriptors_path(home_root: &Path) -> PathBuf {
        home_root
            .join(".pdo")
            .join("harnesses")
            .join("descriptors.yaml")
    }

    /// Load the effective registry from an injected root. Absent or unreadable →
    /// the floor, SILENT (the normal state of an instance). Unparseable, or a
    /// refused row → the floor for that key plus a diagnostic. Never reads `$HOME`,
    /// never writes, never seeds. Emits AT MOST ONE `warn!` per distinct
    /// diagnostic, so a polled spawn/settings path cannot flood the log.
    pub fn load(home_root: &Path) -> Self {
        let path = Self::descriptors_path(home_root);
        let parsed = std::fs::read(&path)
            .ok()
            .map(|b| parse_descriptors(&String::from_utf8_lossy(&b)))
            .unwrap_or_default();
        let registry = Self {
            descriptors: merge_by_name(embedded_floor(), parsed.descriptors),
            disk_rejected: parsed.rejected,
            disk_unparseable: parsed.unparseable,
            disk_path: Some(path),
        };
        registry.warn_once();
        registry
    }

    /// Resolve a name against the merged registry. `None` ⇒ no tier carries that
    /// name (the spawn seam turns this into a fail-fast that names it).
    pub fn resolve(&self, name: &str) -> Option<HarnessDescriptor> {
        self.descriptors.iter().find(|d| d.name == name).cloned()
    }

    /// The names the registry resolves, in declaration order (floor first, then
    /// any novel disk harness). The `GET /settings` "which harnesses exist" view.
    pub fn names(&self) -> Vec<String> {
        self.descriptors.iter().map(|d| d.name.clone()).collect()
    }

    /// The disk descriptors the registry refused — each inert, its key on the
    /// floor. For the settings surface.
    pub fn rejected(&self) -> &[RejectedDescriptor] {
        &self.disk_rejected
    }

    /// ONE message naming an inert descriptor file and every refused row, or
    /// `None` when the disk tier is clean/absent. PURE, so a unit test can pin it
    /// instead of a terminal (modelled on `price_table::PriceTable::diagnostic`).
    pub fn diagnostic(&self) -> Option<String> {
        let where_ = match &self.disk_path {
            Some(p) => format!("harness descriptor tier ({})", p.display()),
            None => "harness descriptor tier".to_string(),
        };
        let mut parts: Vec<String> = Vec::new();
        if let Some(err) = &self.disk_unparseable {
            parts.push(format!("{where_} is entirely inert: {err}"));
        }
        if !self.disk_rejected.is_empty() {
            let rows = self
                .disk_rejected
                .iter()
                .map(|r| format!("`{}` ({})", r.name, r.why))
                .collect::<Vec<_>>()
                .join(", ");
            parts.push(format!(
                "{where_} refused {} descriptor(s), each key falling through to the next tier: \
                 {rows}",
                self.disk_rejected.len()
            ));
        }
        if parts.is_empty() {
            None
        } else {
            Some(format!(
                "harness descriptors (#553) — {}",
                parts.join(" ; ")
            ))
        }
    }

    /// Say the diagnostic at most once per distinct message — a *differently* bad
    /// file hashes differently and is said again. Same posture as
    /// `price_table::PriceTable::warn_once` (a loader called once per request must
    /// not log per request).
    fn warn_once(&self) {
        static LAST_WARNED: Mutex<Option<u64>> = Mutex::new(None);
        let Some(msg) = self.diagnostic() else {
            return;
        };
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(&msg, &mut h);
        let fingerprint = std::hash::Hasher::finish(&h);
        let mut guard = LAST_WARNED.lock().unwrap_or_else(|e| e.into_inner());
        if *guard == Some(fingerprint) {
            return;
        }
        *guard = Some(fingerprint);
        warn!("{msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_carries_claude_and_opencode() {
        assert!(resolve(CLAUDE).is_some());
        assert!(resolve(OPENCODE).is_some());
        assert!(resolve("nope").is_none());
    }

    #[test]
    fn claude_pins_identity_has_effort_and_can_resume() {
        let d = claude();
        assert!(d.pins_session_id(), "claude pins --session-id");
        assert!(d.has_effort_hole(), "claude has an effort axis");
        assert!(d.can_resume(), "claude resumes by --resume/--continue");
    }

    #[test]
    fn opencode_has_no_effort_axis_and_no_identity_pin() {
        let d = opencode();
        assert!(
            !d.has_effort_hole(),
            "opencode has no launch-time effort axis (greys the picker)"
        );
        assert!(
            !d.pins_session_id(),
            "opencode cannot pin a session identity"
        );
        assert!(d.can_resume(), "opencode blind-continues");
        assert!(d.env.is_empty(), "opencode carries no CCR env");
    }

    #[test]
    fn merge_by_name_replaces_a_floor_entry_and_keeps_the_rest() {
        let custom_claude = HarnessDescriptor {
            name: CLAUDE.to_string(),
            binary: "my-claude".to_string(),
            launch: vec!["exec".to_string(), "my-claude".to_string()],
            resume: vec![],
            env: vec![],
        };
        let merged = merge_by_name(embedded_floor(), vec![custom_claude.clone()]);
        // claude is replaced by the disk entry…
        let c = merged.iter().find(|d| d.name == CLAUDE).unwrap();
        assert_eq!(c.binary, "my-claude");
        // …and opencode, absent from the disk tier, survives from the floor.
        assert!(merged.iter().any(|d| d.name == OPENCODE));
    }

    #[test]
    fn merge_by_name_appends_an_unknown_disk_harness() {
        let novel = HarnessDescriptor {
            name: "novel".to_string(),
            binary: "novel".to_string(),
            launch: vec!["exec".to_string(), "novel".to_string()],
            resume: vec![],
            env: vec![],
        };
        let merged = merge_by_name(embedded_floor(), vec![novel]);
        assert!(merged.iter().any(|d| d.name == "novel"));
        assert_eq!(merged.len(), 3);
    }

    // --- the disk tier (#553) ------------------------------------------------

    #[test]
    fn parse_descriptors_accepts_a_valid_custom_harness() {
        let parsed = parse_descriptors(
            "harnesses:\n  my-harness:\n    binary: my-harness\n    launch: [\"exec\", \"my-harness\", \"--auto\", \"--prompt {prompt}\"]\n    resume: [\"exec\", \"my-harness\", \"--auto\", \"--continue\"]\n    env: { FOO: bar }\n",
        );
        assert!(parsed.unparseable.is_none());
        assert!(parsed.rejected.is_empty());
        assert_eq!(parsed.descriptors.len(), 1);
        let d = &parsed.descriptors[0];
        assert_eq!(d.name, "my-harness");
        assert_eq!(d.binary, "my-harness");
        assert_eq!(d.launch.last().unwrap(), "--prompt {prompt}");
        assert_eq!(d.resume, vec!["exec", "my-harness", "--auto", "--continue"]);
        assert_eq!(d.env, vec![("FOO".to_string(), "bar".to_string())]);
    }

    #[test]
    fn parse_descriptors_treats_an_empty_or_comment_only_file_as_an_empty_tier() {
        for text in [
            "",
            "   \n",
            "# nothing yet\n",
            "harnesses:\n",
            "harnesses: {}\n",
        ] {
            let parsed = parse_descriptors(text);
            assert!(parsed.unparseable.is_none(), "text = {text:?}");
            assert!(parsed.descriptors.is_empty());
            assert!(parsed.rejected.is_empty());
        }
    }

    #[test]
    fn parse_descriptors_refuses_a_row_missing_binary_or_launch_whole() {
        // A row without a binary or a launch template is refused ENTIRELY, never
        // partially applied — its key falls through to the next tier.
        let no_binary = parse_descriptors("harnesses:\n  x:\n    launch: [\"exec\", \"x\"]\n");
        assert!(no_binary.descriptors.is_empty());
        assert_eq!(no_binary.rejected.len(), 1);
        assert!(no_binary.rejected[0].why.contains("binary"));

        let no_launch = parse_descriptors("harnesses:\n  x:\n    binary: x\n");
        assert!(no_launch.descriptors.is_empty());
        assert_eq!(no_launch.rejected.len(), 1);
        assert!(no_launch.rejected[0].why.contains("launch"));
    }

    #[test]
    fn parse_descriptors_keeps_the_good_rows_when_one_is_refused() {
        let parsed = parse_descriptors(
            "harnesses:\n  good:\n    binary: good\n    launch: [\"exec\", \"good\"]\n  bad:\n    launch: [\"exec\", \"bad\"]\n",
        );
        assert_eq!(
            parsed.descriptors.len(),
            1,
            "one bad row must not void the file"
        );
        assert_eq!(parsed.descriptors[0].name, "good");
        assert_eq!(parsed.rejected.len(), 1);
        assert_eq!(parsed.rejected[0].name, "bad");
    }

    #[test]
    fn parse_descriptors_reports_broken_yaml_without_descriptors() {
        let parsed = parse_descriptors("harnesses:\n  - this is: [not, a, map\n");
        assert!(parsed.unparseable.is_some());
        assert!(parsed.descriptors.is_empty());
    }

    #[test]
    fn builtin_registry_resolves_the_floor() {
        let reg = HarnessRegistry::builtin();
        assert!(reg.resolve(CLAUDE).is_some());
        assert!(reg.resolve(OPENCODE).is_some());
        assert!(reg.resolve("nope").is_none());
        assert!(reg.diagnostic().is_none());
        assert_eq!(reg.names(), vec![CLAUDE.to_string(), OPENCODE.to_string()]);
    }

    #[test]
    fn load_on_a_root_without_a_descriptor_file_is_the_floor_and_silent() {
        let home = tempfile::tempdir().unwrap();
        let reg = HarnessRegistry::load(home.path());
        assert!(reg.resolve(CLAUDE).is_some());
        assert!(reg.resolve(OPENCODE).is_some());
        assert!(reg.diagnostic().is_none());
        // Nothing is ever seeded on disk.
        assert!(!HarnessRegistry::descriptors_path(home.path()).exists());
    }

    #[test]
    fn load_merges_a_custom_harness_by_name_over_the_floor() {
        // FP: declare a harness PDO does not know, and it resolves.
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "harnesses:\n  my-harness:\n    binary: my-harness\n    launch: [\"exec\", \"my-harness\", \"--auto\"]\n",
        )
        .unwrap();

        let reg = HarnessRegistry::load(home.path());
        let d = reg
            .resolve("my-harness")
            .expect("the custom harness resolves");
        assert_eq!(d.binary, "my-harness");
        // …and the floor survives alongside it (merge by name).
        assert!(reg.resolve(CLAUDE).is_some());
        assert!(reg.resolve(OPENCODE).is_some());
        assert!(reg.names().contains(&"my-harness".to_string()));
    }

    #[test]
    fn a_broken_descriptor_file_leaves_claude_untouched_and_is_diagnosed() {
        // FP: corrupt the file → it becomes inert and diagnosed; `claude` and
        // `opencode` keep working (the whole disk tier falls through to the floor).
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "harnesses:\n  - not: [a, map\n").unwrap();

        let reg = HarnessRegistry::load(home.path());
        // Floor intact, byte-identical to the embedded claude.
        assert_eq!(reg.resolve(CLAUDE), Some(claude()));
        assert_eq!(reg.resolve(OPENCODE), Some(opencode()));
        // …and the corruption is named, pointing at the real file.
        let d = reg.diagnostic().expect("a broken file must be said");
        assert!(
            d.contains("descriptors.yaml"),
            "the path must be named: {d}"
        );
        assert!(d.contains("inert"));
    }

    #[test]
    fn a_refused_row_falls_through_to_the_floor_and_is_named() {
        // A malformed `claude:` override must not un-define claude: the row is
        // refused, its key falls to the embedded floor, and the refusal is named.
        let home = tempfile::tempdir().unwrap();
        let path = HarnessRegistry::descriptors_path(home.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            // No `binary` → refused whole.
            "harnesses:\n  claude:\n    launch: [\"exec\", \"totally-wrong\"]\n",
        )
        .unwrap();

        let reg = HarnessRegistry::load(home.path());
        assert_eq!(
            reg.resolve(CLAUDE),
            Some(claude()),
            "claude stays the floor"
        );
        assert_eq!(reg.rejected().len(), 1);
        assert_eq!(reg.rejected()[0].name, "claude");
        let d = reg.diagnostic().unwrap();
        assert!(d.contains("`claude`") && d.contains("falling through"));
    }

    #[test]
    fn load_never_writes_to_disk() {
        // The read is pure: loading, even with a healthy file, seeds nothing beyond
        // the file the user already wrote (no fetched.json analogue, no baseline).
        let home = tempfile::tempdir().unwrap();
        let dir = HarnessRegistry::descriptors_path(home.path())
            .parent()
            .unwrap()
            .to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            HarnessRegistry::descriptors_path(home.path()),
            "harnesses:\n  x:\n    binary: x\n    launch: [\"exec\", \"x\"]\n",
        )
        .unwrap();
        let before: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        let _ = HarnessRegistry::load(home.path());
        let after: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(before, after, "load must not write any file");
    }
}
