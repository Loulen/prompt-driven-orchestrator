//! Le **plan** d'image d'un Run : quel Dockerfile, quelle source, ou quel ref explicite —
//! résolu une fois, purement, à partir du choix gelé du profil et des deux tiers env injectés.

use std::path::{Path, PathBuf};

use super::{default_dockerfile_path, image_name_for_dockerfile, ImageSource, ProfileImage};
use crate::domain::profile::DEFAULT_PROFILE_IMAGE;

/// Env var overridant la source d'image du **défaut de profil** (tier optionnel). Lue UNE fois au
/// bord, jamais dans le cœur — miroir de [`pdo_kernel::env::DOCKER_CMD_OVERRIDE_ENV`].
///
/// CONSERVÉE par #471 alors que le réglage d'instance disparaît, et ce n'est pas une exception à
/// « on ne garde pas de champ mort » : une instance headless fraîche n'a que des profils virtuels
/// et pas d'UI, donc c'est son SEUL moyen de changer d'image sans POSTer un profil.
pub const IMAGE_SOURCE_ENV: &str = "PDO_SANDBOX_IMAGE_SOURCE";

/// Résolution PURE de la source d'image, `env → défaut de profil` (#471) — testable sans toucher
/// `std::env`, miroir de [`resolve_dockerfile`]. Le défaut n'est pas nommé ici : il appartient à
/// [`crate::domain::profile::DEFAULT_PROFILE_IMAGE`], comme la liste d'entrées de `full`.
pub fn resolve_image_source(env: Option<ImageSource>) -> ImageSource {
    env.unwrap_or(DEFAULT_PROFILE_IMAGE.source)
}

// -- Dockerfile résolu : 3 tiers depuis #471 (profil → env → défaut) ---------

/// Quel tier a choisi le Dockerfile (#431, amputé du tier `stored` par #471). Miroir de la
/// discipline `as_str` d'[`ImageSource`] ; surfacé par la `reason` du `RunFailed` — qui regarde
/// un « no such file » a besoin de savoir QUI l'a dit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockerfileSource {
    /// Le profil de staging du Run (#467) — le tier le plus fort, et le seul qui soit **par
    /// Run** plutôt que par daemon : il est gelé dans `RunStarted`, pas relu à chaque prep.
    Profile,
    /// [`DOCKERFILE_PATH_ENV`] dans l'env du daemon (#471 : l'échappatoire headless).
    Env,
    /// Le Dockerfile seedé, matérialisation de [`crate::domain::profile::DEFAULT_PROFILE_IMAGE`].
    Default,
}

impl DockerfileSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DockerfileSource::Profile => "profile",
            DockerfileSource::Env => "env",
            DockerfileSource::Default => "default",
        }
    }
}

/// Ce que [`ensure_image`] doit FAIRE pour un Run donné, résolu une fois au bord. Deux branches,
/// et l'intérêt de l'enum est justement qu'elles ne se mélangent pas : la première est adressée par
/// contenu (pull et build interchangeables), la seconde ne l'est pas (pull ou rien).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImagePlan {
    /// Le chemin historique : un Dockerfile + une [`ImageSource`] (pull-ou-build). Le ref rendu
    /// est `<nom>:h-<hash>`.
    HashDerived {
        dockerfile: ResolvedDockerfile,
        source: ImageSource,
    },
    /// Un ref registry explicite posé par le profil `profile`. Le ref rendu est le ref demandé.
    /// `profile` n'est là que pour la `reason` : qui lit « failed to pull » doit savoir QUEL profil
    /// l'a demandé, parce que rien d'autre dans le message ne le dit.
    ExplicitRef { image_ref: String, profile: String },
}

impl ImagePlan {
    /// Le ref que ce plan produira si tout va bien, SANS toucher docker — pour la disclosure et
    /// les logs. `None` en hash-dérivé : le tag est le hash d'octets qu'il faut lire sur disque,
    /// et ce module ne fait pas d'IO en douce depuis un accesseur.
    pub fn known_ref(&self) -> Option<&str> {
        match self {
            ImagePlan::HashDerived { .. } => None,
            ImagePlan::ExplicitRef { image_ref, .. } => Some(image_ref),
        }
    }
}

/// Le plan d'image d'un Run, précédence **profil (si posé) → env → défaut de profil**
/// (#467, amputé du tier `stored` par #471). **PUR** : les deux tiers env sont *injectés*, pas lus
/// — c'est [`crate::adapters::env::image_plan_with`] qui les lit, une fois, au bord.
///
/// Ce découpage n'est pas cosmétique. Tant que cette fonction lisait `std::env` elle-même, aucun
/// test ne pouvait couvrir le tier env sans muter l'environnement du binaire de test, ce que le
/// reste du module refuse explicitement (D2 : `cargo test` parallélise). Séparés, la précédence
/// complète se teste sans effet de bord et les *noms* des variables restent couverts par le seul
/// test qui les mute.
///
/// Deux détails qui se plantent si on les bâcle :
/// - un profil `registry` **court-circuite tout** : il n'y a plus ni hash ni Dockerfile dans
///   l'histoire, donc les deux tiers env n'ont plus rien à décider ;
/// - un profil `dockerfile` ne court-circuite **que** le Dockerfile. La source (pull-ou-build)
///   reste résolue par `env → défaut`, et reste inerte dans le cas normal parce que le prédicat
///   de skip-pull porte sur l'EMPLACEMENT (un chemin custom ne peut pas avoir de tag publié en
///   amont). Un profil qui pointe l'emplacement seedé retire donc bien la pull — c'est le même
///   fichier, le même hash, et #431 a déjà tranché que le prédicat n'est pas du tier-math.
pub fn resolve_image_plan(
    profile_name: &str,
    profile_image: Option<&ProfileImage>,
    env_source: Option<ImageSource>,
    env_dockerfile: Option<&str>,
    sandbox_root: &Path,
) -> ImagePlan {
    match profile_image {
        Some(ProfileImage::Registry { image_ref }) => ImagePlan::ExplicitRef {
            image_ref: image_ref.clone(),
            profile: profile_name.to_string(),
        },
        Some(ProfileImage::Dockerfile { path }) => ImagePlan::HashDerived {
            dockerfile: resolve_dockerfile(Some(path), env_dockerfile, sandbox_root),
            source: resolve_image_source(env_source),
        },
        // Le profil ne pose rien : le défaut de profil décide, l'env pouvant l'override
        // (#471). C'est ce bras qui doit produire EXACTEMENT le ref d'avant #471 — pinné par
        // `the_default_profile_image_yields_the_pre_471_image_ref`.
        None => ImagePlan::HashDerived {
            dockerfile: resolve_dockerfile(None, env_dockerfile, sandbox_root),
            source: resolve_image_source(env_source),
        },
    }
}

/// Le Dockerfile que [`ensure_image`] hashe et builde, résolu UNE fois au bord (#431).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDockerfile {
    pub path: PathBuf,
    pub source: DockerfileSource,
    /// Prédicat de skip-pull (ADR-0030 §5, précisé par #431) : porte sur l'EMPLACEMENT par
    /// défaut, **pas** sur le tier — un profil peut épingler le chemin seedé via le picker, et ça
    /// doit continuer à puller. Égalité `PathBuf` volontairement nue :
    /// `canonicalize` est de l'IO, échoue sur un chemin absent, et empoisonnerait la pureté du
    /// résolveur. Mal classer est inoffensif dans les deux sens (un 404 gâché, ou un pull évité
    /// qui aurait 404 de toute façon) — le skip-pull est une optimisation, pas un gate de
    /// correction.
    pub is_default_location: bool,
    /// Nom d'image que ce Dockerfile produit (#466), dérivé de son NOM DE FICHIER par
    /// [`image_name_for_dockerfile`] — `pdo-sandbox` pour la base, `pdo-sandbox-chrome-dev` pour
    /// `Dockerfile.chrome-dev`. Résolu ici, avec le chemin, pour que `ensure_image` **et** la
    /// disclosure `GET /settings` nomment la même image (0 drift, leçon #373).
    pub image_name: String,
}

/// Env var pointant le Dockerfile du **défaut de profil** (tier optionnel, #431). Lue UNE fois au
/// bord, jamais dans le cœur — miroir de [`IMAGE_SOURCE_ENV`], et CONSERVÉE par #471 pour la même
/// raison headless. Ne passe par aucun validateur : c'est l'échappatoire assumée pour un chemin sur
/// volume amovible, et le gate autoritaire reste le `is_file()` d'[`ensure_hash_derived_image`].
pub const DOCKERFILE_PATH_ENV: &str = "PDO_SANDBOX_DOCKERFILE";

/// Résolution 3-tiers **PURE** `profil → env → défaut de profil` — testable sans toucher
/// `std::env` (AC #431, étendue par #467 au tier `profile`, amputée du tier `stored` par #471).
/// Une valeur vide est traitée comme unset à TOUS les tiers.
///
/// Le tier `default` ne nomme pas de chemin en dur : il lit
/// [`crate::domain::profile::DEFAULT_PROFILE_IMAGE`], dont le `dockerfile: None` veut dire
/// « l'emplacement seedé », le seul défaut exprimable — un chemin littéral dans la constante
/// mentirait, puisqu'il dépend de `<sandbox_root>`, donc de `$HOME`.
pub fn resolve_dockerfile(
    profile: Option<&str>,
    env: Option<&str>,
    sandbox_root: &Path,
) -> ResolvedDockerfile {
    let default = match DEFAULT_PROFILE_IMAGE.dockerfile {
        Some(p) => PathBuf::from(p),
        None => default_dockerfile_path(sandbox_root),
    };
    let (path, source) = match profile.filter(|s| !s.is_empty()) {
        Some(p) => (PathBuf::from(p), DockerfileSource::Profile),
        None => match env.filter(|s| !s.is_empty()) {
            Some(p) => (PathBuf::from(p), DockerfileSource::Env),
            None => (default.clone(), DockerfileSource::Default),
        },
    };
    ResolvedDockerfile {
        is_default_location: path == default,
        image_name: image_name_for_dockerfile(&path),
        path,
        source,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::image::{
        dockerfile_hash, local_image_ref, registry_image_ref, BASE_IMAGE_NAME,
    };

    /// Le Dockerfile de base LIVRÉ, celui que l'adaptateur seede.
    const EMBEDDED_DOCKERFILE: &str = include_str!("../../../assets/sandbox/Dockerfile");

    #[test]
    fn image_source_parse_and_resolver() {
        // parse: round-trip + case-insensitive + unknown → None. PURE, no env mutation.
        assert_eq!(ImageSource::parse("registry"), Some(ImageSource::Registry));
        assert_eq!(
            ImageSource::parse("dockerfile"),
            Some(ImageSource::Dockerfile)
        );
        assert_eq!(ImageSource::parse("REGISTRY"), Some(ImageSource::Registry));
        assert_eq!(
            ImageSource::parse("  dockerfile  "),
            Some(ImageSource::Dockerfile)
        );
        assert_eq!(ImageSource::parse("ecr"), None);
        assert_eq!(ImageSource::parse(""), None);
        assert_eq!(ImageSource::Registry.as_str(), "registry");
        assert_eq!(ImageSource::Dockerfile.as_str(), "dockerfile");

        // #471: the resolver is PURE — `env → défaut de profil`, two tiers, no `std::env`.
        assert_eq!(
            resolve_image_source(None),
            DEFAULT_PROFILE_IMAGE.source,
            "unset env ⇒ the profile-defaults constant, and nothing else names that default"
        );
        assert_eq!(
            resolve_image_source(Some(ImageSource::Dockerfile)),
            ImageSource::Dockerfile,
            "AC4: the env tier still changes the default"
        );
        assert_eq!(
            resolve_image_source(Some(ImageSource::Registry)),
            ImageSource::Registry
        );
    }

    /// AC3, la moitié « source » : la constante de défaut de profil vaut EXACTEMENT ce que le tier
    /// `default` des deux réglages retirés produisait — registre hash-dérivé sur le Dockerfile
    /// seedé. Le golden sur le ref complet est
    /// [`the_default_profile_image_yields_the_pre_471_image_ref`].
    #[test]
    fn the_default_profile_image_is_the_pre_471_instance_default() {
        let d = DEFAULT_PROFILE_IMAGE;
        assert_eq!(
            d.source,
            ImageSource::Registry,
            "pre-#471 `image_source.default` était `registry`"
        );
        assert_eq!(
            d.dockerfile, None,
            "pre-#471 `dockerfile_path.default` était l'emplacement seedé, pas un chemin littéral"
        );
    }

    /// AC3, LE golden : un profil qui ne pose pas d'image produit le ref d'avant #471, bit pour
    /// bit. `pdo-sandbox` (nom de la base, #466) + `h-` + les 12 premiers hex du SHA-256 des octets
    /// du Dockerfile embarqué. Le littéral est là exprès : dériver l'attendu du même code que le
    /// sujet ne prouverait rien, alors qu'un ref écrit en dur casse le jour où la résolution du
    /// défaut change de sens — ce que cette issue jure ne pas faire.
    #[test]
    fn the_default_profile_image_yields_the_pre_471_image_ref() {
        let root = Path::new("/home/u/.pdo/sandbox");
        // Le plan d'un profil qui ne pose rien, tiers env vides comme sur l'instance de référence.
        let plan = resolve_image_plan("full", None, None, None, root);
        let ImagePlan::HashDerived { dockerfile, source } = plan else {
            panic!("un profil sans image doit rester hash-dérivé");
        };
        assert_eq!(dockerfile.path, default_dockerfile_path(root));
        assert_eq!(dockerfile.source, DockerfileSource::Default);
        assert!(dockerfile.is_default_location, "donc la pull reste tentée");
        assert_eq!(dockerfile.image_name, BASE_IMAGE_NAME);
        assert_eq!(source, ImageSource::Registry);
        // Le ref que `ensure_hash_derived_image` rendra pour ces octets, en dur.
        assert_eq!(
            local_image_ref(&dockerfile.image_name, EMBEDDED_DOCKERFILE.as_bytes()),
            format!(
                "pdo-sandbox:h-{}",
                dockerfile_hash(EMBEDDED_DOCKERFILE.as_bytes())
            )
        );
        assert_eq!(
            registry_image_ref(&dockerfile.image_name, EMBEDDED_DOCKERFILE.as_bytes()),
            format!(
                "ghcr.io/loulen/pdo-sandbox:h-{}",
                dockerfile_hash(EMBEDDED_DOCKERFILE.as_bytes())
            ),
            "et c'est ce ref GHCR que la pull vise, comme avant"
        );
    }

    // -- #431 : le Dockerfile résolu est un réglage à tiers (3 depuis #471) ----

    #[test]
    fn resolve_dockerfile_precedence_profile_env_default() {
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);

        // default tier: nothing posed by the profile, nothing in env.
        let r = resolve_dockerfile(None, None, root);
        assert_eq!(r.path, default);
        assert_eq!(r.source, DockerfileSource::Default);
        assert!(r.is_default_location);

        // #467: the PROFILE tier beats the env one.
        let r = resolve_dockerfile(Some("/profile/Dockerfile"), Some("/env/Dockerfile"), root);
        assert_eq!(r.path, Path::new("/profile/Dockerfile"));
        assert_eq!(r.source, DockerfileSource::Profile);
        // …and an empty profile value is unset at that tier too, like every other one.
        let r = resolve_dockerfile(Some(""), Some("/env/Dockerfile"), root);
        assert_eq!(r.source, DockerfileSource::Env);

        // env tier wins over the default (AC4: still true after #471).
        let r = resolve_dockerfile(None, Some("/env/Dockerfile"), root);
        assert_eq!(r.path, Path::new("/env/Dockerfile"));
        assert_eq!(r.source, DockerfileSource::Env);
        assert!(!r.is_default_location);
    }

    #[test]
    fn resolve_dockerfile_treats_empty_string_as_unset_at_both_tiers() {
        // An empty value must never win precedence at either surviving tier.
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);

        let r = resolve_dockerfile(None, Some(""), root);
        assert_eq!(r.path, default);
        assert_eq!(r.source, DockerfileSource::Default);

        let r = resolve_dockerfile(Some(""), Some(""), root);
        assert_eq!(r.path, default);
        assert_eq!(r.source, DockerfileSource::Default);
    }

    #[test]
    fn is_default_location_is_about_the_path_not_the_tier() {
        // THE tier-vs-path trap: pinning the DEFAULT path through the profile picker names a
        // tier — but the location is still the seeded one, so the pull must still be attempted.
        // `is_default_location` is path-math, not tier-math.
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);
        let r = resolve_dockerfile(Some(default.to_str().unwrap()), None, root);
        assert_eq!(r.source, DockerfileSource::Profile);
        assert!(
            r.is_default_location,
            "a profile pointing AT the default location must still pull"
        );
        // Same, through the env tier.
        let r = resolve_dockerfile(None, Some(default.to_str().unwrap()), root);
        assert_eq!(r.source, DockerfileSource::Env);
        assert!(r.is_default_location);
    }

    #[test]
    fn dockerfile_source_as_str_round_trips() {
        assert_eq!(DockerfileSource::Profile.as_str(), "profile");
        assert_eq!(DockerfileSource::Env.as_str(), "env");
        assert_eq!(DockerfileSource::Default.as_str(), "default");
    }

    #[test]
    fn resolve_dockerfile_carries_the_variant_image_name() {
        let root = Path::new("/home/u/.pdo/sandbox");
        // Le tier `default` (Dockerfile seedé) → nom de base.
        assert_eq!(
            resolve_dockerfile(None, None, root).image_name,
            BASE_IMAGE_NAME
        );
        // Un chemin pointant la variante → nom de variante, SANS 3e valeur d'enum
        // [`ImageSource`] : c'est le chemin de sélection le plus court (#466 périmètre pt 4).
        let r = resolve_dockerfile(
            None,
            Some("/repo/assets/sandbox/Dockerfile.chrome-dev"),
            root,
        );
        assert_eq!(r.image_name, "pdo-sandbox-chrome-dev");
        assert_eq!(r.source, DockerfileSource::Env);
        assert!(
            !r.is_default_location,
            "la variante n'est pas à l'emplacement seedé → build local, jamais de pull"
        );
    }

    fn profile_dockerfile(path: &Path) -> ProfileImage {
        ProfileImage::Dockerfile {
            path: path.to_str().unwrap().to_string(),
        }
    }

    /// Le plan d'un profil, tiers env explicitement vides — le cas nominal.
    fn plan_of(profile_name: &str, image: Option<&ProfileImage>, root: &Path) -> ImagePlan {
        resolve_image_plan(profile_name, image, None, None, root)
    }

    /// La précédence de #467 telle que #471 la laisse, sur les trois cas qui comptent. Tiers env
    /// vides ici (voir `the_env_tiers_override_the_profile_default` pour l'autre moitié), donc le
    /// tier `default` est celui de la couche de défauts de profil.
    #[test]
    fn image_plan_puts_the_profile_first() {
        let root = Path::new("/home/u/.pdo/sandbox");

        // (a) le profil ne pose rien → le défaut de profil, c.-à-d. le Dockerfile seedé.
        let plan = plan_of("full", None, root);
        assert_eq!(
            plan,
            ImagePlan::HashDerived {
                dockerfile: resolve_dockerfile(None, None, root),
                source: ImageSource::Registry,
            }
        );
        assert_eq!(
            plan.known_ref(),
            None,
            "le tag hash-dérivé n'est pas connu sans IO"
        );

        // (b) profil `dockerfile` → il gagne le tier ; la source reste résolue env→défaut.
        let plan = plan_of(
            "chrome",
            Some(&ProfileImage::Dockerfile {
                path: "/repo/Dockerfile.chrome-dev".to_string(),
            }),
            root,
        );
        match plan {
            ImagePlan::HashDerived { dockerfile, source } => {
                assert_eq!(dockerfile.path, Path::new("/repo/Dockerfile.chrome-dev"));
                assert_eq!(dockerfile.source, DockerfileSource::Profile);
                // #466 : le NOM d'image suit le nom de fichier, y compris par ce chemin-là.
                assert_eq!(dockerfile.image_name, "pdo-sandbox-chrome-dev");
                assert_eq!(source, ImageSource::Registry);
                assert!(
                    !dockerfile.is_default_location,
                    "un chemin custom n'a pas de tag publié en amont → build local"
                );
            }
            other => panic!("expected a hash-derived plan, got {other:?}"),
        }

        // (c) profil `registry` → court-circuite tout : plus ni hash ni Dockerfile.
        let plan = plan_of(
            "chrome",
            Some(&ProfileImage::Registry {
                image_ref: "ghcr.io/acme/agent:1.4".to_string(),
            }),
            root,
        );
        assert_eq!(
            plan,
            ImagePlan::ExplicitRef {
                image_ref: "ghcr.io/acme/agent:1.4".to_string(),
                profile: "chrome".to_string(),
            }
        );
        assert_eq!(plan.known_ref(), Some("ghcr.io/acme/agent:1.4"));
    }

    /// AC4 : les deux tiers ENV changent encore le défaut, et un profil qui pose une image gagne
    /// toujours sur eux. Entièrement PUR grâce au découpage `resolve_image_plan` / `image_plan_with`
    /// — avant #471 ce test n'était pas écrivable sans muter l'environnement du binaire de test.
    #[test]
    fn the_env_tiers_override_the_profile_default() {
        let root = Path::new("/home/u/.pdo/sandbox");
        let env_df = "/env/Dockerfile.from-env";

        // (a) profil muet + les deux tiers env → l'env décide des DEUX moitiés du plan.
        let plan = resolve_image_plan(
            "full",
            None,
            Some(ImageSource::Dockerfile),
            Some(env_df),
            root,
        );
        match &plan {
            ImagePlan::HashDerived { dockerfile, source } => {
                assert_eq!(dockerfile.path, Path::new(env_df));
                assert_eq!(dockerfile.source, DockerfileSource::Env);
                assert_eq!(*source, ImageSource::Dockerfile);
            }
            other => panic!("expected a hash-derived plan, got {other:?}"),
        }

        // (b) un profil `dockerfile` bat `DOCKERFILE_PATH_ENV`…
        let plan = resolve_image_plan(
            "chrome",
            Some(&ProfileImage::Dockerfile {
                path: "/repo/Dockerfile.chrome-dev".to_string(),
            }),
            Some(ImageSource::Dockerfile),
            Some(env_df),
            root,
        );
        match &plan {
            ImagePlan::HashDerived { dockerfile, source } => {
                assert_eq!(dockerfile.path, Path::new("/repo/Dockerfile.chrome-dev"));
                assert_eq!(dockerfile.source, DockerfileSource::Profile);
                // …et ne touche PAS à la source, qui reste celle de l'env.
                assert_eq!(*source, ImageSource::Dockerfile);
            }
            other => panic!("expected a hash-derived plan, got {other:?}"),
        }

        // (c) un profil `registry` bat les DEUX : plus de Dockerfile, plus de source à décider.
        assert_eq!(
            resolve_image_plan(
                "chrome",
                Some(&ProfileImage::Registry {
                    image_ref: "ghcr.io/acme/agent:1.4".to_string(),
                }),
                Some(ImageSource::Dockerfile),
                Some(env_df),
                root,
            ),
            ImagePlan::ExplicitRef {
                image_ref: "ghcr.io/acme/agent:1.4".to_string(),
                profile: "chrome".to_string(),
            },
            "un ref explicite ne consulte aucun tier env"
        );
    }

    /// Un profil qui pointe l'emplacement SEEDÉ garde la pull : le prédicat de skip-pull porte sur
    /// l'emplacement, pas sur le tier (#431), et ni #467 ni #471 ne changent cette règle.
    #[test]
    fn a_profile_dockerfile_at_the_default_location_still_pulls() {
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);
        let plan = plan_of("chrome", Some(&profile_dockerfile(&default)), root);
        match plan {
            ImagePlan::HashDerived { dockerfile, .. } => {
                assert_eq!(dockerfile.source, DockerfileSource::Profile);
                assert!(
                    dockerfile.is_default_location,
                    "même fichier, même hash : la pull reste légitime"
                );
            }
            other => panic!("expected a hash-derived plan, got {other:?}"),
        }
    }
}
