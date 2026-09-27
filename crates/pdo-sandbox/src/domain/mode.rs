//! Whether a Run is sandboxed, and in which staging profile: the [`SandboxMode`],
//! and the precedence that picks it.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// How a Run is isolated (#403 / #407 / #432). A **per-Run, immutable** property
/// carried on `RunStarted`, projected once into the Run's state (`RunState::sandbox`), never mutated
/// for the Run's whole life — a resumed session matches its transcript by working-dir
/// path, so flipping the mode mid-life would break `claude --continue`.
///
/// `full` and `minimal` are *virtual defaults* (no DB row until edited), so they keep
/// round-tripping byte-identically through every historical payload.
///
/// The wire form is a **bare string**: `off`, or the profile name verbatim. Serde is
/// hand-written for exactly that reason — `untagged` would emit `null` for the unit
/// variant, and `#[serde(from = "String")]` would demand an infallible conversion
/// while a blank token must fail.
///
/// [`SandboxMode::parse`] is purely **syntactic**: `None` means *blank*, not
/// *unknown*. Whether a profile **exists** is a database question, answered at the
/// edge (create-run, `PUT /settings`, trigger create/patch) and never here — this
/// module is pure and its projection runs inside `append_event`.
/// On the wire it is a plain string (`off` or the profile name), hence the
/// TypeScript override.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, TS)]
#[ts(type = "string")]
pub enum SandboxMode {
    #[default]
    Off,
    /// Canonicalised by [`SandboxMode::parse`]: trimmed, never empty, never `off`.
    Profile(String),
}

impl SandboxMode {
    /// The default tier (never `None`), surfaced by `GET /settings` and used as the
    /// precedence floor: an install with no `default_sandbox` set runs `Off`, so the
    /// legacy host path stays byte-identical (#410). Mirror of the image source default.
    pub const DEFAULT: SandboxMode = SandboxMode::Off;

    /// The wire token of [`SandboxMode::Off`]. A `const &str` rather than
    /// `DEFAULT.as_str()` because `as_str` now borrows from `self` (the profile name
    /// is owned), which no `const fn` can do.
    pub const OFF_WIRE: &'static str = "off";

    /// Whether this Run runs on the host (the legacy, no-Docker path). The whole
    /// sandbox wiring is gated on `!is_off()`, so the `off` parcours never touches
    /// a single new line.
    pub fn is_off(&self) -> bool {
        matches!(self, SandboxMode::Off)
    }

    /// The exact wire form: `off`, or the profile name verbatim. Consumed by
    /// `build_settings_view` and the enum validators (#410).
    pub fn as_str(&self) -> &str {
        match self {
            SandboxMode::Off => Self::OFF_WIRE,
            SandboxMode::Profile(name) => name.as_str(),
        }
    }

    /// The staging profile this Run uses, or `None` for `off`. The ONE consumer that
    /// needs the name rather than the off-ness (#432 D2) is the sandbox context
    /// assembly, which resolves it to a frozen entry list.
    pub fn profile(&self) -> Option<&str> {
        match self {
            SandboxMode::Off => None,
            SandboxMode::Profile(name) => Some(name.as_str()),
        }
    }

    /// Parse the wire form. **Purely syntactic** (#432): `off` (case/whitespace
    /// tolerant, the three closed tokens of the old enum are gone) yields `Off`, a
    /// blank string yields `None`, and anything else is a profile name — trimmed but
    /// otherwise **verbatim**, never lowercased.
    ///
    /// The asymmetry with [`crate::domain::profile::validate::validate_profile_name`] (which
    /// rejects an uppercase name outright instead of folding it) is deliberate and
    /// load-bearing: `off` is a closed token whose spelling nobody owns, while a
    /// profile name is a user namespace where accepting `Foo` and silently storing
    /// `foo` would make the UI search a list it does not display. Do not "fix" it.
    pub fn parse(s: &str) -> Option<SandboxMode> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return None;
        }
        if trimmed.eq_ignore_ascii_case(Self::OFF_WIRE) {
            return Some(SandboxMode::Off);
        }
        Some(SandboxMode::Profile(trimmed.to_string()))
    }
}

impl Serialize for SandboxMode {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SandboxMode {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(d)?;
        SandboxMode::parse(&raw).ok_or_else(|| {
            serde::de::Error::custom(
                "sandbox must be `off` or the name of a staging profile, not blank",
            )
        })
    }
}

/// Instance default, precedence `stored → env → default(Off)`. A stored empty value is
/// treated as unset. Pure: the env tier is *injected* (read once at the edge by
/// [`crate::adapters::env::default_sandbox_with`]).
///
/// A stored profile name that does not *exist* is deliberately NOT demoted to `off`
/// here: it wins the tier and the create-run chokepoint 400s on it by name (ADR-0031
/// §7 — never a silent fallback toward less isolation).
pub fn resolve_default_sandbox(stored: Option<String>, env: Option<SandboxMode>) -> SandboxMode {
    stored
        .filter(|s| !s.is_empty())
        .as_deref()
        .and_then(SandboxMode::parse)
        .or(env)
        .unwrap_or(SandboxMode::DEFAULT)
}

/// Precedence resolver (#410): `explicit → trigger → instance_default` (first `Some`
/// wins, `instance_default` is the floor). Pure — no `AppState`/DB/Docker in scope;
/// this is the layer-1 unit the "précédence testée" AC pins. `explicit` and `trigger`
/// are mutually exclusive in production (a Run has one origin), but the 3-arg form is
/// the canonical statement of the chain and keeps every arm exercised by the test.
pub fn effective_sandbox(
    explicit: Option<SandboxMode>,
    trigger: Option<SandboxMode>,
    instance_default: SandboxMode,
) -> SandboxMode {
    explicit.or(trigger).unwrap_or(instance_default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> SandboxMode {
        SandboxMode::Profile("full".into())
    }
    fn minimal() -> SandboxMode {
        SandboxMode::Profile("minimal".into())
    }

    #[test]
    fn explicit_off_beats_full_default() {
        // The bug #410 exists to fix: an explicit `off` must survive a `full`/`minimal`
        // instance default — otherwise the default could never be overridden downward.
        assert_eq!(
            effective_sandbox(Some(SandboxMode::Off), None, full()),
            SandboxMode::Off
        );
    }

    #[test]
    fn explicit_wins_over_trigger_and_default() {
        assert_eq!(
            effective_sandbox(Some(minimal()), Some(full()), SandboxMode::Off),
            minimal()
        );
    }

    #[test]
    fn trigger_used_when_no_explicit() {
        assert_eq!(
            effective_sandbox(None, Some(minimal()), SandboxMode::Off),
            minimal()
        );
        // A trigger's explicit `off` also stands over a `full` instance default.
        assert_eq!(
            effective_sandbox(None, Some(SandboxMode::Off), full()),
            SandboxMode::Off
        );
    }

    #[test]
    fn all_none_falls_to_instance_default() {
        assert_eq!(effective_sandbox(None, None, full()), full());
        assert_eq!(
            effective_sandbox(None, None, SandboxMode::DEFAULT),
            SandboxMode::Off
        );
    }

    #[test]
    fn default_sandbox_precedence() {
        // stored valid wins, over the env too.
        assert_eq!(
            resolve_default_sandbox(Some("minimal".into()), None),
            minimal()
        );
        assert_eq!(
            resolve_default_sandbox(Some("full".into()), Some(minimal())),
            full()
        );
        // empty sentinel → unset → env, else default (Off).
        assert_eq!(
            resolve_default_sandbox(Some(String::new()), None),
            SandboxMode::Off
        );
        assert_eq!(
            resolve_default_sandbox(Some(String::new()), Some(full())),
            full()
        );
        // absent → default.
        assert_eq!(resolve_default_sandbox(None, None), SandboxMode::Off);
    }

    /// #432, the behaviour change worth pinning: an unrecognised stored token is NO
    /// LONGER demoted to `off`. It is a PROFILE NAME, it wins the tier, and the
    /// create-run chokepoint 400s on it by name (ADR-0031 §7 — never a silent fallback
    /// toward less isolation). The resolver is pure and cannot know whether the
    /// profile exists; that is the edge's job.
    #[test]
    fn a_stored_unknown_name_stays_the_winning_tier() {
        assert_eq!(
            resolve_default_sandbox(Some("full-no-mcp".into()), None),
            SandboxMode::Profile("full-no-mcp".into())
        );
        // Even the two pre-#426 tokens: they are just names now, and they will fail loud
        // at launch instead of quietly demoting the whole instance to the host path.
        assert_eq!(
            resolve_default_sandbox(Some("copy".into()), None),
            SandboxMode::Profile("copy".into())
        );
    }

    #[test]
    fn sandbox_mode_parse_and_as_str_round_trip() {
        for mode in [SandboxMode::Off, full(), minimal()] {
            assert_eq!(SandboxMode::parse(mode.as_str()), Some(mode));
        }
        // `off` is case / whitespace tolerant (a closed token nobody owns the spelling of).
        assert_eq!(SandboxMode::parse("  OFF "), Some(SandboxMode::Off));
        // A profile name is trimmed but NEVER lowercased — see the asymmetry documented on
        // `parse` and on `validate_profile_name`.
        assert_eq!(
            SandboxMode::parse("  Full-No-MCP "),
            Some(SandboxMode::Profile("Full-No-MCP".into()))
        );
        // Blank is the ONLY `None`: `parse` is syntactic since #432, existence is a
        // database question answered at the edge.
        assert_eq!(SandboxMode::parse(""), None);
        assert_eq!(SandboxMode::parse("   "), None);
        assert_eq!(
            SandboxMode::parse("nope"),
            Some(SandboxMode::Profile("nope".into()))
        );
    }

    /// The wire form is a BARE STRING, byte-identical to the pre-#432 enum for the two
    /// names that existed. Every historical `RunStarted` payload must round-trip
    /// unchanged, which is what makes this a type change and not a data migration.
    #[test]
    fn sandbox_mode_serialises_as_a_bare_string() {
        for wire in ["off", "full", "minimal", "full-no-mcp"] {
            let parsed = SandboxMode::parse(wire).unwrap();
            let json = serde_json::to_value(&parsed).unwrap();
            assert_eq!(json, serde_json::json!(wire), "{wire} must round-trip");
            assert_eq!(
                serde_json::from_value::<SandboxMode>(json).unwrap(),
                parsed,
                "{wire} must deserialise back"
            );
        }
        // A blank / non-string value fails deserialisation rather than becoming `Off`.
        assert!(serde_json::from_value::<SandboxMode>(serde_json::json!("")).is_err());
        assert!(serde_json::from_value::<SandboxMode>(serde_json::json!(null)).is_err());
        assert!(serde_json::from_value::<SandboxMode>(serde_json::json!(3)).is_err());
    }
}
