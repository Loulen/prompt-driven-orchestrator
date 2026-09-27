//! What a sandboxed Run **froze** at creation (ADR-0031 §6): its profile's entry
//! list, env and image source, as the Run's projection carries them — and the
//! decision tables that turn each into what the container gets.

use std::collections::BTreeMap;

use anyhow::Result;

use crate::domain::image::ProfileImage;
use crate::domain::profile::VIRTUAL_PROFILES;

/// The sandbox half of a Run's projection: each frozen key, and the raw value when
/// it was present but unreadable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrozenSandbox {
    pub run_id: String,
    pub entries: Option<Vec<String>>,
    pub entries_raw_error: Option<String>,
    pub env: Option<BTreeMap<String, String>>,
    pub env_raw_error: Option<String>,
    pub image: Option<ProfileImage>,
    pub image_raw_error: Option<String>,
}

/// What [`frozen_entries`] decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrozenEntries {
    /// The list the Run froze, verbatim.
    Frozen(Vec<String>),
    /// No frozen list, but the profile is a virtual default: resolve it now.
    ReResolveVirtual,
}

/// Resolve the entry list a sandboxed Run must stage, from its **frozen** projection
/// (#432, ADR-0031 §6). The decision table, and why each arm is what it is:
///
/// | payload | behaviour | why |
/// |---|---|---|
/// | name + valid entries | **entries verbatim**, the name is ignored | the pure form of the freeze. A profile deleted since is *not* an error — surviving that is the point |
/// | `full`/`minimal`, **no** entries | resolve the virtual default now | a virtual default always resolves; `RunFailed` on a perfectly resolvable Run is indefensible |
/// | a user profile, **no** entries | **hard error** → `RunFailed` | unreachable by construction (the one chokepoint writes both keys) |
/// | entries present but unreadable | **hard error**, raw value in the reason | undecidable; any fallback silently changes what the already-spawned nodes saw |
///
/// The `full`/`minimal`-without-entries arm does, in effect, read the **live** setting.
/// Assumed and documented (ADR-0031 §6 note): it only fires for a Run created by a
/// pre-profiles daemon, cleaned up, then resumed. Both alternatives are worse
/// (`RunFailed` on something resolvable; or freezing the #426 default into Rust for
/// ever, which contradicts ADR-0031 §2).
pub fn frozen_entries(frozen: &FrozenSandbox, name: &str) -> Result<FrozenEntries> {
    if let Some(raw) = &frozen.entries_raw_error {
        anyhow::bail!(
            "run {} froze an unreadable sandbox entry list ({raw}); refusing to re-resolve \
             a different list for a Run whose nodes already staged one",
            frozen.run_id
        );
    }
    if let Some(entries) = &frozen.entries {
        // Frozen list wins outright — even if the profile has been edited or deleted.
        return Ok(FrozenEntries::Frozen(entries.clone()));
    }
    // No frozen list: a pre-#432 payload. Only a virtual default may be re-resolved.
    if !VIRTUAL_PROFILES.contains(&name) {
        anyhow::bail!(
            "run {} names the staging profile `{name}` but froze no entry list \
             (payload predates #432 and `{name}` is not a built-in default); refusing to \
             guess what it should stage",
            frozen.run_id
        );
    }
    Ok(FrozenEntries::ReResolveVirtual)
}

/// Resolve the env a sandboxed Run must pose at `docker create`, from its **frozen**
/// projection (#468, ADR-0031 §8).
///
/// | payload | behaviour | why |
/// |---|---|---|
/// | `sandbox_env` present and readable | **verbatim** | the freeze. A profile edited (or deleted) since is not an error — surviving that is the point |
/// | absent | **empty** | a pre-#468 daemon could pose no profile env at all, so absence and emptiness describe the same container. Reading the live profile here would *add* variables to a Run in flight — the exact retroactivity §6 forbids |
/// | present but unreadable | **hard error** | undecidable; posing "no env" instead would start the container without the variables its MCP servers need and look like a plugin bug |
///
/// The [`frozen_entries`] twin has a fourth row — re-resolving a *virtual default* whose list
/// is absent — and this one deliberately has not. There, the alternative was `RunFailed` on
/// a perfectly resolvable Run; here, "no env" is not a guess, it is what the Run actually
/// ran with.
///
/// The error message names the raw value, like `frozen_entries` does: this arm can only fire
/// on a payload that is not a map of strings, i.e. one that cannot be carrying the user's
/// values in the first place.
pub fn frozen_env(frozen: &FrozenSandbox) -> Result<BTreeMap<String, String>> {
    if let Some(raw) = &frozen.env_raw_error {
        anyhow::bail!(
            "run {} froze an unreadable sandbox env ({raw}); refusing to start a container \
             with a different environment than the Run's nodes already saw",
            frozen.run_id
        );
    }
    Ok(frozen.env.clone().unwrap_or_default())
}

/// Resolve the image source a sandboxed Run's **profile** froze at creation (#467, ADR-0031 §9).
/// A strict copy of [`frozen_env`]'s three rows — same decision table, same reasons:
///
/// | payload | behaviour | why |
/// |---|---|---|
/// | `sandbox_image` present and readable | **verbatim** | the freeze. A profile whose image was edited (or deleted) since is not an error — surviving that is the point |
/// | absent | **`None`** = "the profile posed none" | a pre-#467 daemon could pose none at all, so absence and "poses none" describe the same resolution: the profile default decides (`DEFAULT_PROFILE_IMAGE`, #471), exactly as before |
/// | present but unreadable | **hard error** | undecidable; falling back to the profile default would start the container in a DIFFERENT image than the nodes that already launched ran in — the one failure this freeze exists to prevent |
///
/// Note what `None` does **not** mean: it is not "no image". Every sandboxed Run has an image;
/// `None` only says the profile did not choose it.
pub fn frozen_image(frozen: &FrozenSandbox) -> Result<Option<ProfileImage>> {
    if let Some(raw) = &frozen.image_raw_error {
        anyhow::bail!(
            "run {} froze an unreadable sandbox image source ({raw}); refusing to start a \
             container from a different image than the Run's nodes already ran in",
            frozen.run_id
        );
    }
    Ok(frozen.image.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_map(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn frozen() -> FrozenSandbox {
        FrozenSandbox {
            run_id: "r1".into(),
            entries: Some(Vec::new()),
            ..FrozenSandbox::default()
        }
    }

    #[test]
    fn a_frozen_entry_list_wins_whatever_the_profile_now_says() {
        let f = FrozenSandbox {
            entries: Some(vec![".gitconfig".into()]),
            ..frozen()
        };
        assert_eq!(
            frozen_entries(&f, "deleted-since").unwrap(),
            FrozenEntries::Frozen(vec![".gitconfig".into()])
        );
    }

    #[test]
    fn only_a_virtual_default_is_re_resolved_without_a_frozen_list() {
        let f = FrozenSandbox {
            entries: None,
            ..frozen()
        };
        assert_eq!(
            frozen_entries(&f, "full").unwrap(),
            FrozenEntries::ReResolveVirtual
        );
        let err = frozen_entries(&f, "team").unwrap_err();
        assert!(format!("{err:#}").contains("refusing to guess"), "{err:#}");
    }

    #[test]
    fn an_unreadable_entry_list_fails_loud() {
        let f = FrozenSandbox {
            entries_raw_error: Some("42".into()),
            ..frozen()
        };
        let msg = format!("{:#}", frozen_entries(&f, "full").unwrap_err());
        assert!(msg.contains("42") && msg.contains("unreadable"), "{msg}");
    }

    /// Row 1 of the decision table: a frozen env is used **verbatim**, whatever the store
    /// now says. That is what makes an edit non-retroactive.
    #[test]
    fn frozen_env_is_used_verbatim() {
        let f = FrozenSandbox {
            env: Some(env_map(&[("FOO", "bar")])),
            ..frozen()
        };
        assert_eq!(frozen_env(&f).unwrap(), env_map(&[("FOO", "bar")]));
    }

    /// Row 2, and the asymmetry with [`frozen_entries`]: an ABSENT env is not a legacy arm
    /// to re-resolve, it is "no env". Re-resolving would add variables to a Run in flight.
    #[test]
    fn an_absent_frozen_env_is_empty_not_re_resolved() {
        assert!(frozen_env(&frozen()).unwrap().is_empty());
        let f = FrozenSandbox {
            env: Some(BTreeMap::new()),
            ..frozen()
        };
        assert!(frozen_env(&f).unwrap().is_empty());
    }

    /// Row 3: unreadable is a HARD error naming the raw value, never a silent "no env" —
    /// which would start the container without the variables its MCP servers need and look
    /// like a plugin bug.
    #[test]
    fn an_unreadable_frozen_env_fails_loud() {
        let f = FrozenSandbox {
            env_raw_error: Some("42".to_string()),
            ..frozen()
        };
        let msg = format!("{:#}", frozen_env(&f).unwrap_err());
        assert!(
            msg.contains("42"),
            "the reason must name the raw value: {msg}"
        );
        assert!(msg.contains("unreadable"), "{msg}");
    }

    /// Row 1: a frozen image source is used **verbatim**, whatever the store now says. That is
    /// what makes an edit non-retroactive — the AC2 guarantee, at the unit level.
    #[test]
    fn frozen_image_is_used_verbatim() {
        let img = ProfileImage::Registry {
            image_ref: "ghcr.io/acme/agent:1.4".to_string(),
        };
        let f = FrozenSandbox {
            image: Some(img.clone()),
            ..frozen()
        };
        assert_eq!(frozen_image(&f).unwrap(), Some(img));
    }

    /// Row 2: an ABSENT source means "the profile posed none" — NOT "no image". The profile
    /// default then decides, exactly as before #467.
    #[test]
    fn an_absent_frozen_image_is_none_not_an_error() {
        assert_eq!(frozen_image(&frozen()).unwrap(), None);
    }

    /// Row 3: unreadable is a HARD error naming the raw value, never a silent fallback to the
    /// profile default — which would start the container in a DIFFERENT image than the nodes
    /// that already launched ran in.
    #[test]
    fn an_unreadable_frozen_image_fails_loud() {
        let f = FrozenSandbox {
            image_raw_error: Some("{\"kind\":\"ecr\"}".to_string()),
            ..frozen()
        };
        let msg = format!("{:#}", frozen_image(&f).unwrap_err());
        assert!(
            msg.contains("ecr"),
            "the reason must name the raw value: {msg}"
        );
        assert!(msg.contains("unreadable"), "{msg}");
    }
}
