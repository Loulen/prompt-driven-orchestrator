//! Resolve an agentic-harness name to its descriptor (ADR-0045).
//!
//! A **harness** is the program that runs a NodeRun's agent (`claude`,
//! `opencode`, …). It declares itself with two argv-token templates (launch,
//! resume), an env block, and the binary PDO probes at spawn — never with named
//! per-feature fields (a named field per case becomes a spelling per case; the
//! template covers them without naming, ADR-0045).
//!
//! This module never reads `$HOME` (the discipline `run_cost` paid for in #408 — a
//! root goes in, a descriptor comes out). [`HarnessRegistry::from_disk_tier`] takes the
//! user-declared descriptor file read under an **injected root**, parses it, and merges
//! it over the embedded floor **by name** ([`merge_by_name`]). Nothing is ever
//! written or seeded. A descriptor that is unreadable or refused is **inert and
//! diagnosed**: its key falls through to the next tier (the floor), it is never
//! partially applied, and it is named — once per distinct diagnostic in the log,
//! and always in `GET /settings` — the exact idiom of `price_table` (ADR-0034).

mod builtins;
mod descriptor;
mod file;

use std::path::{Path, PathBuf};

pub use builtins::{claude, copilot, embedded_floor, opencode, pi};
pub use descriptor::*;
pub use file::{
    launch_makes_binary_leader, parse_descriptors, ParsedDescriptors, RejectedDescriptor,
};

/// Merge a user-declared disk tier over the embedded floor, **by name**: a disk
/// descriptor replaces the floor's entry of the same name; a floor name absent
/// from disk survives. Pure — the caller reads and parses the disk tier and hands
/// the descriptors in.
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
pub fn resolve(name: &str) -> Option<HarnessDescriptor> {
    embedded_floor().into_iter().find(|d| d.name == name)
}

/// Where a resolved harness came from — the embedded floor, or the disk tier
/// (#586). The picker splits its two sections on this fact; it is a name-based
/// judgement (`claude`/`opencode` are the floor), so a disk *override* of a floor
/// name stays `Builtin` — the picker still lists it under "Built-in", which is
/// what the design shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HarnessSource {
    Builtin,
    Descriptor,
}

impl HarnessSource {
    /// The wire token `GET /settings` surfaces (`builtin` | `descriptor`).
    pub fn as_str(self) -> &'static str {
        match self {
            HarnessSource::Builtin => "builtin",
            HarnessSource::Descriptor => "descriptor",
        }
    }
}

/// One resolved harness as the picker needs it (#586): its name, its provenance,
/// and the binary PDO probes at spawn. The binary is exposed — not an `installed`
/// bool — because whether it resolves on `$PATH` is decided by the caller
/// (`adapters::binary_path::binary_available`, which alone reads the environment);
/// this pure module never touches `$PATH`, the same discipline it keeps for
/// `$HOME`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessListing {
    pub name: String,
    pub source: HarnessSource,
    pub binary: String,
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

    /// The registry of a disk tier read at `path`: `text` is the file's content,
    /// `None` when it is absent or unreadable (the floor, silent — the normal state
    /// of an instance). Unparseable, or a refused row → the floor for that key plus
    /// a diagnostic. Pure: the read is the caller's (`application::load_registry`).
    pub fn from_disk_tier(path: PathBuf, text: Option<&str>) -> Self {
        let parsed = text.map(parse_descriptors).unwrap_or_default();
        Self {
            descriptors: merge_by_name(embedded_floor(), parsed.descriptors),
            disk_rejected: parsed.rejected,
            disk_unparseable: parsed.unparseable,
            disk_path: Some(path),
        }
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

    /// Each resolved harness with its provenance and probe binary, in resolution
    /// order (#586). `source` is `Builtin` for the embedded floor names
    /// (`claude`/`opencode`), `Descriptor` for a disk-declared harness — the split
    /// the picker renders as two sections. `installed` is NOT decided here: the
    /// caller pairs each `binary` with `adapters::binary_path::binary_available`, so
    /// this module stays pure (no `$PATH`). Refused rows never enter
    /// `self.descriptors`, so they are already absent — a refused key resolves to
    /// the floor, which is what appears.
    pub fn listing(&self) -> Vec<HarnessListing> {
        let builtin: std::collections::HashSet<String> =
            embedded_floor().into_iter().map(|d| d.name).collect();
        self.descriptors
            .iter()
            .map(|d| HarnessListing {
                name: d.name.clone(),
                source: if builtin.contains(&d.name) {
                    HarnessSource::Builtin
                } else {
                    HarnessSource::Descriptor
                },
                binary: d.binary.clone(),
            })
            .collect()
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
        assert!(
            d.has_settings_hole(),
            "claude accepts an injected settings file (the Stop-hook substrate)"
        );
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
        assert!(
            !d.has_settings_hole(),
            "opencode has no settings hole — PDO writes it no settings file (#613)"
        );
    }

    #[test]
    fn copilot_is_on_the_floor_pins_identity_and_resumes_by_identity_only() {
        assert!(resolve(COPILOT).is_some());
        let d = copilot();
        assert_eq!(d.binary, "copilot");
        assert!(d.launch.iter().any(|t| t == "--allow-all"));
        assert!(d.launch.iter().any(|t| t == "--no-ask-user"));
        assert!(d.launch.iter().any(|t| t.contains("{model}")));
        assert!(d.pins_session_id(), "copilot pins --session-id");
        assert!(d.can_resume());
        // Resume is by identity or not at all — never a blind continue (AC).
        assert_eq!(d.resume_by_id, "--resume");
        assert!(
            d.resume_blind.is_empty(),
            "copilot never blind-continues (AC)"
        );
        assert_eq!(
            d.env,
            vec![("COPILOT_AUTO_UPDATE".to_string(), "false".to_string())]
        );
    }

    #[test]
    fn pi_is_on_the_floor_imposes_identity_and_resumes_by_the_same_flag_only() {
        // #705: the third first-party harness. Resident TUI, project files
        // pre-approved, imposed session identity, model, effort, positional prompt.
        assert!(resolve(PI).is_some());
        let d = pi();
        assert_eq!(d.binary, "pi");
        assert!(
            d.launch.iter().any(|t| t == "-a"),
            "project files pre-approved"
        );
        assert!(d.launch.iter().any(|t| t.contains("{model}")));
        assert!(d.has_effort_hole(), "pi has an effort axis (--thinking)");
        assert!(d.pins_session_id(), "pi imposes --session-id");
        assert!(
            d.launch.last().is_some_and(|t| t == "{prompt}"),
            "positional prompt"
        );
        assert!(
            !d.launch.iter().any(|t| t == "-p" || t == "--print"),
            "never the one-shot mode: it exits at turn end (ineligible, ADR-0032)"
        );
        // The `{settings}` hole carries the `-e` extension token (turn-end extension,
        // #707) — present in the shape, empty in this ticket.
        assert!(d.has_settings_hole());
        assert!(d.launch.iter().any(|t| t == "-e {settings}"));
        assert!(d.resume.iter().any(|t| t == "-e {settings}"));
        // Resume by identity only, with the SAME created-or-resumed flag; never blind.
        assert!(d.can_resume());
        assert_eq!(d.resume_by_id, "--session-id");
        assert!(d.resume_blind.is_empty(), "pi never blind-continues");
        assert!(
            d.resume.iter().any(|t| t.contains("{effort}")),
            "effort is re-posed on resume from the start event"
        );
        assert_eq!(
            d.env,
            vec![
                ("PI_SKIP_VERSION_CHECK".to_string(), "1".to_string()),
                ("PI_TELEMETRY".to_string(), "0".to_string()),
            ]
        );
    }

    #[test]
    fn the_whole_floor_makes_its_binary_the_pane_leader() {
        // ADR-0054: PDO's one descriptor invariant holds for every embedded harness
        // — each launch begins `exec <binary> …`, so the harness leads the pane.
        for d in embedded_floor() {
            assert!(
                launch_makes_binary_leader(&d.launch, &d.binary),
                "{} must make its binary the pane leader",
                d.name
            );
        }
    }

    #[test]
    fn every_embedded_harness_names_a_validated_version() {
        // #617: the support table names the binary each row describes. A harness on
        // the floor with no validated version would publish a row about nothing —
        // so the floor and the version table are kept in step by this test rather
        // than by memory.
        for d in embedded_floor() {
            let v = validated_version(&d.name)
                .unwrap_or_else(|| panic!("{} must name a validated version", d.name));
            assert!(!v.trim().is_empty(), "{} version must not be blank", d.name);
        }
        // A data-declared harness has validated nothing.
        assert_eq!(validated_version("my-custom-harness"), None);
    }

    #[test]
    fn resume_verbs_are_the_floor_harnesses_own_property() {
        assert_eq!(claude().resume_by_id, "--resume");
        assert_eq!(claude().resume_blind, "--continue");
        // opencode cannot pin identity → blind-continue only.
        assert!(opencode().resume_by_id.is_empty());
        assert_eq!(opencode().resume_blind, "--continue");
    }

    #[test]
    fn parse_descriptors_refuses_a_launch_that_does_not_lead_the_pane() {
        // ADR-0054: a launch that wraps the binary in a shell (or omits `exec`) is
        // refused whole, same shape as a missing `binary`/`launch`; its key falls
        // through to the next tier.
        let wrapped = parse_descriptors(
            "harnesses:\n  x:\n    binary: x\n    launch: [\"bash\", \"-lc\", \"exec x\"]\n",
        );
        assert!(wrapped.descriptors.is_empty());
        assert_eq!(wrapped.rejected.len(), 1);
        assert_eq!(wrapped.rejected[0].name, "x");
        assert!(wrapped.rejected[0].why.contains("pane leader"));

        // Missing `exec` — the binary is launched but the shell stays leader.
        let no_exec =
            parse_descriptors("harnesses:\n  x:\n    binary: x\n    launch: [\"x\", \"--auto\"]\n");
        assert_eq!(no_exec.rejected.len(), 1);
        assert!(no_exec.rejected[0].why.contains("pane leader"));

        // `exec` present but leading a DIFFERENT binary than declared.
        let wrong_leader = parse_descriptors(
            "harnesses:\n  x:\n    binary: x\n    launch: [\"exec\", \"y\", \"--auto\"]\n",
        );
        assert_eq!(wrong_leader.rejected.len(), 1);
        assert!(wrong_leader.rejected[0].why.contains("pane leader"));
    }

    #[test]
    fn parse_descriptors_carries_the_resume_verbs() {
        // A disk descriptor may declare its resume verbs (#614), filling `{resume}`.
        let parsed = parse_descriptors(
            "harnesses:\n  h:\n    binary: h\n    launch: [\"exec\", \"h\", \"{prompt}\"]\n    resume: [\"exec\", \"h\", \"{resume}\"]\n    resume_by_id: \"--resume\"\n    resume_blind: \"--continue\"\n",
        );
        assert_eq!(parsed.descriptors.len(), 1);
        assert_eq!(parsed.descriptors[0].resume_by_id, "--resume");
        assert_eq!(parsed.descriptors[0].resume_blind, "--continue");
    }

    #[test]
    fn merge_by_name_replaces_a_floor_entry_and_keeps_the_rest() {
        let custom_claude = HarnessDescriptor {
            name: CLAUDE.to_string(),
            binary: "my-claude".to_string(),
            launch: vec!["exec".to_string(), "my-claude".to_string()],
            resume: vec![],
            resume_by_id: String::new(),
            resume_blind: String::new(),
            env: vec![],
        };
        let merged = merge_by_name(embedded_floor(), vec![custom_claude.clone()]);
        let c = merged.iter().find(|d| d.name == CLAUDE).unwrap();
        assert_eq!(c.binary, "my-claude");
        assert!(merged.iter().any(|d| d.name == OPENCODE));
    }

    #[test]
    fn merge_by_name_appends_an_unknown_disk_harness() {
        let novel = HarnessDescriptor {
            name: "novel".to_string(),
            binary: "novel".to_string(),
            launch: vec!["exec".to_string(), "novel".to_string()],
            resume: vec![],
            resume_by_id: String::new(),
            resume_blind: String::new(),
            env: vec![],
        };
        let merged = merge_by_name(embedded_floor(), vec![novel]);
        assert!(merged.iter().any(|d| d.name == "novel"));
        assert_eq!(merged.len(), embedded_floor().len() + 1);
    }

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
        assert_eq!(
            reg.names(),
            vec![
                CLAUDE.to_string(),
                OPENCODE.to_string(),
                COPILOT.to_string(),
                PI.to_string()
            ]
        );
    }

    #[test]
    fn builtin_listing_is_the_floor_as_builtin() {
        let listing = HarnessRegistry::builtin().listing();
        assert_eq!(listing.len(), 4);
        assert!(listing.iter().all(|h| h.source == HarnessSource::Builtin));
        assert_eq!(listing[0].name, CLAUDE);
        assert_eq!(listing[1].name, OPENCODE);
        assert_eq!(listing[2].name, COPILOT);
        assert_eq!(listing[3].name, PI);
    }
}
