//! The environment variables the sandbox reads, each once, at the edge: the two
//! image tiers of the profile default (#471) and the instance default sandbox
//! (#410). The resolvers they feed are pure ([`crate::domain`]).

use std::path::Path;

use crate::domain::image::{
    resolve_image_plan, ImagePlan, ImageSource, ProfileImage, DOCKERFILE_PATH_ENV, IMAGE_SOURCE_ENV,
};
use crate::domain::mode::{resolve_default_sandbox, SandboxMode};

/// Env var overriding the stored instance default (optional tier). Read ONCE at the
/// edge (create-run chokepoint + the settings page's disclosure), never in the
/// resolver core — mirror of [`IMAGE_SOURCE_ENV`] (#410).
pub const DEFAULT_SANDBOX_ENV: &str = "PDO_DEFAULT_SANDBOX";

/// Env tier for the settings disclosure / resolver: `Some(mode)` if a valid
/// `PDO_DEFAULT_SANDBOX` is set, else `None` (unset/invalid).
pub fn env_default_sandbox() -> Option<SandboxMode> {
    std::env::var(DEFAULT_SANDBOX_ENV)
        .ok()
        .as_deref()
        .and_then(SandboxMode::parse)
}

/// Instance default, precedence `stored → env → default(Off)`. SINGLE source shared by
/// the create-run chokepoint AND the settings page (`instance_defaults`). See
/// [`resolve_default_sandbox`].
pub fn default_sandbox_with(stored: Option<String>) -> SandboxMode {
    resolve_default_sandbox(stored, env_default_sandbox())
}

/// Le tier env de la source d'image. Lu UNE fois au bord, par [`image_plan_with`].
pub fn env_image_source() -> Option<ImageSource> {
    std::env::var(IMAGE_SOURCE_ENV)
        .ok()
        .as_deref()
        .and_then(ImageSource::parse)
}

/// Le tier env du Dockerfile. Une valeur vide est traitée comme unset.
pub fn env_dockerfile_path() -> Option<String> {
    std::env::var(DOCKERFILE_PATH_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Wrapper de bord de [`resolve_image_plan`] : lit les DEUX variables d'env une fois chacune, puis
/// délègue. SOURCE UNIQUE consommée par l'assemblage du contexte sandbox d'un Run, donc elle voit
/// exactement les mêmes tiers que ce que `ensure_image` consommera (0 drift, leçon #373).
pub fn image_plan_with(
    profile_name: &str,
    profile_image: Option<&ProfileImage>,
    sandbox_root: &Path,
) -> ImagePlan {
    resolve_image_plan(
        profile_name,
        profile_image,
        env_image_source(),
        env_dockerfile_path().as_deref(),
        sandbox_root,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::image::DockerfileSource;

    /// Les deux *noms* de variables, et rien d'autre. LE seul test du module qui mute
    /// `std::env` — toléré ici, et seulement ici, parce que depuis #471 plus aucun autre test du
    /// workspace ne lit ces deux variables : la précédence passe par `resolve_image_plan` (pur), et
    /// `context_from_state` n'est exercé que par des tests d'intégration, qui sont d'autres
    /// processus. Restaure ce qu'il trouve, pour ne pas empoisonner un binaire de test réutilisé.
    #[test]
    fn the_edge_wrappers_read_the_documented_env_var_names() {
        let root = Path::new("/home/u/.pdo/sandbox");
        let before_src = std::env::var(IMAGE_SOURCE_ENV).ok();
        let before_df = std::env::var(DOCKERFILE_PATH_ENV).ok();

        std::env::set_var(IMAGE_SOURCE_ENV, "dockerfile");
        std::env::set_var(DOCKERFILE_PATH_ENV, "/env/Dockerfile.named");
        assert_eq!(env_image_source(), Some(ImageSource::Dockerfile));
        assert_eq!(
            env_dockerfile_path().as_deref(),
            Some("/env/Dockerfile.named")
        );
        // And the edge wrapper threads both into the plan.
        match image_plan_with("full", None, root) {
            ImagePlan::HashDerived { dockerfile, source } => {
                assert_eq!(dockerfile.path, Path::new("/env/Dockerfile.named"));
                assert_eq!(dockerfile.source, DockerfileSource::Env);
                assert_eq!(source, ImageSource::Dockerfile);
            }
            other => panic!("expected a hash-derived plan, got {other:?}"),
        }

        // An unknown token is unset, not a panic and not a third variant.
        std::env::set_var(IMAGE_SOURCE_ENV, "ecr");
        assert_eq!(env_image_source(), None);
        // An empty path is unset too (the sentinel discipline the resolvers share).
        std::env::set_var(DOCKERFILE_PATH_ENV, "");
        assert_eq!(env_dockerfile_path(), None);

        match before_src {
            Some(v) => std::env::set_var(IMAGE_SOURCE_ENV, v),
            None => std::env::remove_var(IMAGE_SOURCE_ENV),
        }
        match before_df {
            Some(v) => std::env::set_var(DOCKERFILE_PATH_ENV, v),
            None => std::env::remove_var(DOCKERFILE_PATH_ENV),
        }
    }

    /// The default-sandbox env tier, by its documented name — the one test that
    /// touches `PDO_DEFAULT_SANDBOX`, restoring what it found.
    #[test]
    fn the_default_sandbox_env_tier_is_read_by_name() {
        let before = std::env::var(DEFAULT_SANDBOX_ENV).ok();
        std::env::set_var(DEFAULT_SANDBOX_ENV, "minimal");
        assert_eq!(
            default_sandbox_with(None),
            SandboxMode::Profile("minimal".into())
        );
        // A stored value still wins over the env.
        assert_eq!(default_sandbox_with(Some("off".into())), SandboxMode::Off);
        std::env::set_var(DEFAULT_SANDBOX_ENV, "   ");
        assert_eq!(env_default_sandbox(), None);
        match before {
            Some(v) => std::env::set_var(DEFAULT_SANDBOX_ENV, v),
            None => std::env::remove_var(DEFAULT_SANDBOX_ENV),
        }
    }
}
