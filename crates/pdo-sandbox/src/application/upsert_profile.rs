//! Create or edit a staging profile: store its diff (ADR-0031 §2).

use std::path::Path;

use pdo_kernel::{Actor, Verb};

use super::profiles::{ProfileError, SandboxProfiles};
use crate::domain::image::ProfileImage;
use crate::domain::profile::draft::{
    check_no_clash, normalise_disabled, normalise_env, ProfileDraft,
};
use crate::domain::profile::validate::{
    validate_entry, validate_profile_image, validate_profile_name,
};
use crate::domain::profile::ResolvedProfile;

impl SandboxProfiles {
    /// Store `draft` as the diff of the profile `name`, and return what it resolves to.
    ///
    /// `upsert` and not create-then-update: the caller cannot know whether `full` already
    /// has a row, and ADR-0031 §2 says editing it *is* what materialises one. The
    /// Operation is a creation when no row exists yet, an update otherwise.
    ///
    /// Validation, all fail-fast before anything is written:
    /// - the name against the grammar (`off` and `""` reserved, no case folding);
    /// - each extra through `validate_entry`, and it must **exist under `$HOME` right
    ///   now** — an early UX gate, necessary but never sufficient, since the staging
    ///   warns-and-skips a path that vanishes later;
    /// - each `disabled` name by **membership in the built-in default**;
    /// - `extras ∩ disabled = ∅`;
    /// - each env key and value, the run-constant keys PDO poses itself being a
    ///   **400 naming the key**, never a silent skip;
    /// - the image source — an absolute path for `dockerfile`, **which must exist**, and a
    ///   pullable-looking ref for `registry`, deliberately NOT probed: PDO cannot check
    ///   that a ref resolves without a network round-trip inside a PUT, nor that the
    ///   image contains an agent at all. Whoever supplies the ref owns that (ADR-0030
    ///   pt 7) — which is also why a failed pull is a hard error rather than a build.
    pub async fn upsert(
        &self,
        actor: &Actor,
        name: &str,
        draft: &ProfileDraft,
    ) -> Result<ResolvedProfile, ProfileError> {
        let name = validate_profile_name(name).map_err(ProfileError::Invalid)?;
        let home = self.home.home();

        let mut extras: Vec<String> = Vec::with_capacity(draft.extras.len());
        for raw in &draft.extras {
            let norm = validate_entry(raw).map_err(ProfileError::Invalid)?;
            if let Some(home) = home.as_deref() {
                let abs = home.join(&norm);
                if !self.home.exists(&abs) {
                    return Err(ProfileError::Invalid(format!(
                        "`{norm}`: nothing at {} — pick a file or folder that exists under \
                         your home directory",
                        abs.display()
                    )));
                }
            }
            if !extras.contains(&norm) {
                extras.push(norm);
            }
        }
        extras.sort();

        let disabled = normalise_disabled(&draft.disabled).map_err(ProfileError::Invalid)?;
        check_no_clash(&disabled, &extras).map_err(ProfileError::Invalid)?;
        let env = normalise_env(&draft.env).map_err(ProfileError::Invalid)?;

        // The grammar first, then — for a Dockerfile — an early existence gate. Necessary,
        // never sufficient: the authoritative check is `ensure_image`'s `is_file()` bail at
        // prep time, since the file can vanish or sit on an unmounted volume. Catching it
        // here turns "the Run died three minutes in" into "the form said no".
        let image = match draft.image.as_ref() {
            None => None,
            Some(raw) => match validate_profile_image(raw).map_err(ProfileError::Invalid)? {
                ProfileImage::Dockerfile { path } => {
                    if !self.home.is_file(Path::new(&path)) {
                        return Err(ProfileError::Invalid(format!(
                            "`{path}`: nothing at that path, or it is not a regular file — pick \
                             the Dockerfile this profile should build its image from"
                        )));
                    }
                    Some(ProfileImage::Dockerfile { path })
                }
                other => Some(other),
            },
        };

        match self.store.get(&name).await? {
            Some(_) => self.authorize(actor, Verb::Update, Some(&name))?,
            None => self.authorize(actor, Verb::Create, None)?,
        }
        let profile = ProfileDraft {
            disabled,
            extras,
            env,
            image,
        };
        self.store.upsert(&name, &profile).await?;
        match self.resolve(&name).await {
            Ok(Some(resolved)) => Ok(resolved),
            Ok(None) | Err(_) => Err(ProfileError::Store(
                "profile written but not readable back".to_string(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pdo_kernel::{Actor, ResourceKind, Verb};

    use super::*;
    use crate::application::profiles::fixture::{refusing, with_home};

    fn draft() -> ProfileDraft {
        ProfileDraft {
            disabled: vec![".claude/plugins".into()],
            extras: vec![" .gitconfig ".into(), ".gitconfig".into()],
            env: BTreeMap::from([(" FOO ".to_string(), "bar".to_string())]),
            image: None,
        }
    }

    #[tokio::test]
    async fn a_valid_draft_is_normalised_stored_and_resolved() {
        let f = with_home(&["/home/u/.gitconfig"]);
        let team = f
            .profiles
            .upsert(&Actor::LocalOperator, "team", &draft())
            .await
            .unwrap();
        assert_eq!(team.extras, vec![".gitconfig"]);
        assert_eq!(team.disabled, vec![".claude/plugins"]);
        assert_eq!(team.env["FOO"], "bar");
        assert!(team.materialised);
        assert_eq!(f.store.names(), vec!["team"]);
    }

    #[tokio::test]
    async fn an_extra_missing_under_home_is_refused_before_anything_is_written() {
        let f = with_home(&[]);
        let err = f
            .profiles
            .upsert(&Actor::LocalOperator, "team", &draft())
            .await
            .unwrap_err();
        assert!(
            matches!(&err, ProfileError::Invalid(m) if m.contains("/home/u/.gitconfig")),
            "{err:?}"
        );
        assert!(f.store.names().is_empty());
    }

    #[tokio::test]
    async fn a_reserved_name_and_a_missing_dockerfile_are_invalid() {
        let f = with_home(&["/home/u/.gitconfig"]);
        let err = f
            .profiles
            .upsert(&Actor::LocalOperator, "off", &ProfileDraft::default())
            .await
            .unwrap_err();
        assert!(matches!(err, ProfileError::Invalid(_)));
        let image = ProfileDraft {
            image: Some(ProfileImage::Dockerfile {
                path: "/nope/Dockerfile".into(),
            }),
            ..ProfileDraft::default()
        };
        let err = f
            .profiles
            .upsert(&Actor::LocalOperator, "team", &image)
            .await
            .unwrap_err();
        assert!(matches!(&err, ProfileError::Invalid(m) if m.contains("/nope/Dockerfile")));
    }

    #[tokio::test]
    async fn a_refusal_writes_nothing_and_names_a_creation_then_an_update() {
        let (f, host) = refusing();
        let err = f
            .profiles
            .upsert(&Actor::LocalOperator, "team", &ProfileDraft::default())
            .await
            .unwrap_err();
        assert!(matches!(&err, ProfileError::Denied(d) if d.reason == "admins only"));
        assert!(f.store.names().is_empty());
        let (_, op) = host.asked().pop().unwrap();
        assert_eq!(
            (op.resource, op.verb, op.target),
            (ResourceKind::SandboxProfile, Verb::Create, None)
        );

        f.store.seed(crate::domain::profile::ProfileDiff {
            name: "team".into(),
            disabled: Vec::new(),
            extras: Vec::new(),
            env: BTreeMap::new(),
            image: None,
            updated_at: "t".into(),
        });
        let agent = Actor::Agent {
            run_id: "r1".into(),
            node_id: "n1".into(),
        };
        f.profiles
            .upsert(&agent, "team", &ProfileDraft::default())
            .await
            .unwrap_err();
        let (actor, op) = host.asked().pop().unwrap();
        assert_eq!(actor, agent);
        assert_eq!(
            (op.verb, op.target.as_deref()),
            (Verb::Update, Some("team"))
        );
    }
}
