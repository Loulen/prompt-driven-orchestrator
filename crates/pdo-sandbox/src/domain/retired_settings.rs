//! The two instance settings #471 retired: `image_source` (#411) and
//! `dockerfile_path` (#431). A staging profile's own image source replaced
//! both, and a value left in either is now **inert** — which must never happen
//! in silence.

use super::image::{DOCKERFILE_PATH_ENV, IMAGE_SOURCE_ENV};

/// The retired fields, ordered as the boot warning should read them, each
/// paired with the env var that survived — the only remaining instance-wide
/// way to move that same knob.
pub const RETIRED_SANDBOX_IMAGE_COLUMNS: &[(&str, &str)] = &[
    ("image_source", IMAGE_SOURCE_ENV),
    ("dockerfile_path", DOCKERFILE_PATH_ENV),
];

/// The ONE boot line owed to a user whose instance still stores a retired
/// value, or `None` when nothing is stored — "nothing stored ⇒ nothing said" is
/// a property of the value, not of a branch at the call site. ONE line for both
/// fields: two lines for one obsolete decision would read like two problems.
///
/// `profile_hint` is the profile a Run currently falls back to — the concrete
/// thing to go and edit. `None` degrades to naming the screen instead.
pub fn retired_settings_warning(
    stored: &[(&'static str, String)],
    profile_hint: Option<&str>,
) -> Option<String> {
    if stored.is_empty() {
        return None;
    }
    let values = stored
        .iter()
        .map(|(field, value)| format!("`{field}` = `{value}`"))
        .collect::<Vec<_>>()
        .join(", ");
    let where_to_go = match profile_hint {
        Some(name) => format!("the staging profile `{name}`, which is this instance's default"),
        None => "the staging profile your Runs launch with".to_string(),
    };
    let env_vars = RETIRED_SANDBOX_IMAGE_COLUMNS
        .iter()
        .map(|(_, env)| format!("`{env}`"))
        .collect::<Vec<_>>()
        .join(" / ");
    Some(format!(
        "instance_config still stores {values}, and #471 made those values IGNORED — a staging \
         profile's own image source decides now. Set the image on {where_to_go} (Settings → \
         Manage staging profiles…), or set {env_vars} in the daemon's environment to move the \
         built-in default. No Run is affected either way."
    ))
}

/// The message a `PUT /settings` gets for a retired field. It has to answer the
/// only question the caller has — *where did it go?* — hence naming the profile
/// editor AND the surviving env var; a bare "unknown field" would be true and
/// useless.
pub fn retired_setting_message(field: &str, env_var: &str) -> String {
    format!(
        "`{field}` is no longer an instance-wide setting: a staging profile's own image source \
         replaced it (#471). Set the image on the profile itself (Settings → Manage staging \
         profiles…), or set `{env_var}` in the daemon's environment to move the built-in default."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_stored_says_nothing() {
        assert_eq!(retired_settings_warning(&[], Some("team")), None);
    }

    #[test]
    fn the_warning_names_the_values_and_where_to_go() {
        let stored = vec![
            ("image_source", "dockerfile".to_string()),
            ("dockerfile_path", "/repo/Dockerfile".to_string()),
        ];
        let msg = retired_settings_warning(&stored, Some("chrome")).unwrap();
        // ONE line, both values, verbatim.
        assert!(msg.contains("`image_source` = `dockerfile`"), "{msg}");
        assert!(
            msg.contains("`dockerfile_path` = `/repo/Dockerfile`"),
            "{msg}"
        );
        assert!(!msg.contains('\n'), "one line, not two: {msg}");
        // It says the values are ignored, names the profile to edit, and the env escape hatch.
        assert!(msg.contains("IGNORED"), "{msg}");
        assert!(msg.contains("`chrome`"), "the profile to edit: {msg}");
        assert!(msg.contains(IMAGE_SOURCE_ENV), "{msg}");
        assert!(msg.contains(DOCKERFILE_PATH_ENV), "{msg}");
        // And that no Run is affected — the whole point of it being a warning, not an error.
        assert!(msg.contains("No Run is affected"), "{msg}");

        // With no default profile (`off`), it names the screen instead of a profile.
        let msg = retired_settings_warning(&stored, None).unwrap();
        assert!(
            msg.contains("the staging profile your Runs launch with"),
            "{msg}"
        );
    }

    #[test]
    fn a_put_names_the_field_and_its_surviving_env_var() {
        let msg = retired_setting_message("image_source", IMAGE_SOURCE_ENV);
        assert!(msg.contains("`image_source` is no longer"), "{msg}");
        assert!(msg.contains(IMAGE_SOURCE_ENV), "{msg}");
    }
}
