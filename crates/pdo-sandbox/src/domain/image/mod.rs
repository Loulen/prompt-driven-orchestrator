//! Fourniture de l'image sandbox (#405 build local, slice B du PRD #403 ; #411 pull GHCR hybride).
//!
//! Le contexte garantit qu'une image `pdo-sandbox:h-<hash>` (tag = hash du CONTENU du
//! Dockerfile) existe localement. [`crate::application::image::ensure_image`] est **hybride** (#411) : si l'image
//! n'est pas déjà locale et que la source est [`ImageSource::Registry`] (défaut), il
//! `docker pull ghcr.io/loulen/pdo-sandbox:h-<hash>` puis retague vers le ref local ;
//! un pull raté (offline / tag absent / registry down) retombe sur le build local, et
//! [`ImageSource::Dockerfile`] build directement sans jamais tirer. La valeur de retour
//! reste TOUJOURS le ref local `pdo-sandbox:h-<hash>` (le même [`dockerfile_hash`] côté
//! pull et build), donc [`crate::domain::container`] est inchangé. Les slices sœurs le
//! CONSOMMENT :
//! - #406 instancie un conteneur à partir de l'image ;
//! - #407 câble [`crate::application::image::ensure_image`] dans le run-advance (ADR-0030) — via `spawn_blocking`
//!   car `docker build`/`docker pull` sont bloquants et longs.
//!
//! Le tag est **adressé par contenu**, pas versionné :
//! rationale (content-hash vs semver ; interchangeabilité pull #411 / build local)
//! -> ADR-0030 (#407, pt 7).
//!
//! Depuis #466 le **nom** de l'image est une donnée de la **variante** de Dockerfile, dérivée de
//! son nom de fichier par [`image_name_for_dockerfile`] : la base `Dockerfile` donne
//! `pdo-sandbox`, la variante `Dockerfile.chrome-dev` (node + Chrome + chrome-devtools-mcp) donne
//! `pdo-sandbox-chrome-dev`. Le hash, lui, ne dépend que des octets — nom et tag varient donc
//! ensemble et indépendamment, et deux variantes ne peuvent pas se recouvrir.
//!
//! Depuis #467 la **source** de l'image appartient au profil de staging
//! ([`ProfileImage`]), et [`crate::application::image::ensure_image`] devient un aiguillage sur un [`ImagePlan`] à **deux**
//! branches qui ne partagent presque rien :
//! - [`ImagePlan::HashDerived`] — tout ce qui précède : le tag EST le hash des octets d'un
//!   Dockerfile, donc pull et build sont interchangeables et un pull raté retombe sur un build ;
//! - [`ImagePlan::ExplicitRef`] — un ref registry libre posé par un profil. Il n'a **pas** de
//!   Dockerfile, donc pas de hash, donc **aucun repli build** : un pull en échec est une erreur
//!   DURE, et le ref local est le ref tel quel (jamais de retag en `h-<hash>`). C'est l'amendement
//!   #467 d'ADR-0030 pt 7.
//!
//! Depuis #471 le profil est le **seul** endroit où ce choix se fait : les deux réglages
//! d'instance (`image_source`, `dockerfile_path`) sont retirés, et ce qu'un profil qui ne pose
//! rien résout est devenu une **constante de défaut de profil**
//! ([`crate::domain::profile::DEFAULT_PROFILE_IMAGE`], à côté de `DEFAULT_FULL_ENTRIES`) —
//! registre hash-dérivé sur le Dockerfile seedé, exactement ce que le tier `default` des deux
//! réglages produisait. Les deux tiers ENV ([`IMAGE_SOURCE_ENV`], [`DOCKERFILE_PATH_ENV`]) sont
//! CONSERVÉS et repointés sur ce défaut : une instance headless n'a que des profils virtuels et
//! pas d'UI, donc l'env est son seul moyen de changer d'image sans POSTer un profil.

//!
//! Ce module est le **vocabulaire pur** de l'image : hash, noms, refs, sources, et le plan
//! résolu ([`plan`]). Les effets docker vivent dans [`crate::application::image`], derrière le
//! port [`crate::application::ports::DockerCli`] ; la lecture de l'env dans
//! [`crate::adapters::env`].

pub mod plan;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ts_rs::TS;

pub use plan::{
    resolve_dockerfile, resolve_image_plan, resolve_image_source, DockerfileSource, ImagePlan,
    ResolvedDockerfile, DOCKERFILE_PATH_ENV, IMAGE_SOURCE_ENV,
};

/// Message d'erreur unique quand le binaire `docker` est introuvable sur le PATH. Devient la
/// `reason` d'un `RunFailed` (US-16) : jamais d'exécution silencieuse sur l'hôte.
/// Réutilisé verbatim par le cycle de vie du conteneur (#406) — un seul message canonique
/// docker-absent partagé par l'image et le conteneur.
pub const DOCKER_NOT_FOUND_MSG: &str =
    "sandbox run requires Docker, but the `docker` binary was not found on PATH — \
     install Docker or set this run's sandbox to `off`";

/// Message when the `docker` binary IS present but its daemon is unreachable (#410).
/// The precise reason the availability probe grays out `full`/`minimal` in the UI. This
/// case and `DOCKER_NOT_FOUND_MSG` both collapse to `available: false` — no action is
/// gated on the distinction (the probe is advisory; the run-advance fail-fast, ADR-0030
/// pt 4, stays the authoritative gate) — so the cause survives only in `reason`.
pub const DOCKER_DAEMON_UNREACHABLE_MSG: &str =
    "the `docker` binary is present but the Docker daemon is unreachable — \
     start Docker or set this run's sandbox to `off`";

/// Advisory availability of a usable Docker for sandboxed Runs (#410). `available`
/// gates nothing on its own (the run-advance fail-fast is authoritative); it drives
/// the NewRunModal's greying of `full`/`minimal`. `reason` carries the human-readable
/// cause when unavailable (one of the two messages above), `None` when available.
#[derive(Debug, Clone)]
pub struct DockerProbe {
    pub available: bool,
    pub reason: Option<String>,
}

// -- path math (pur, sans IO) ------------------------------------------------

/// `<sandbox_root>/Dockerfile` — emplacement canonique du Dockerfile **seedé**, et le
/// tier `default` de [`resolve_dockerfile`] (#431). Renommé depuis `dockerfile_path` :
/// depuis que le chemin est réglable, un `dockerfile_path` nu serait un nom menteur.
pub fn default_dockerfile_path(sandbox_root: &Path) -> PathBuf {
    sandbox_root.join("Dockerfile")
}

/// `<sandbox_root>/.build-ctx` — contexte de build dédié, gardé VIDE (cf. D8) : `~/.pdo/sandbox/`
/// a pour siblings les staging dirs par-run (`<run-id>/claude-home/`, ~98 Mo, écrits
/// concurremment) — l'utiliser comme contexte enverrait un tarball géant et racerait un run.
pub fn build_context_dir(sandbox_root: &Path) -> PathBuf {
    sandbox_root.join(".build-ctx")
}

// -- hash / tag (pur ; SINGLE SOURCE OF TRUTH pour #411 + release.yml) --------

/// SHA-256 sur les octets EXACTS du Dockerfile fed à `docker build`, 12 hex minuscules.
/// Équivalent CI CANONIQUE : `sha256sum Dockerfile | cut -c1-12`. Hasher les octets bruts ;
/// **jamais normaliser** (pas de conversion `\r\n`, pas de fix de newline finale) — le
/// hash-input DOIT == build-input, sinon réutilisation d'une image périmée (#411 hashe en bash).
pub fn dockerfile_hash(dockerfile_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dockerfile_bytes);
    let full = format!("{:x}", hasher.finalize());
    full[..12].to_string()
}

/// Nom d'image de la variante de BASE (`assets/sandbox/Dockerfile`).
pub const BASE_IMAGE_NAME: &str = "pdo-sandbox";

/// Nom d'image dérivé du NOM DE FICHIER du Dockerfile résolu (#466) — path math pure, zéro IO.
///
/// `Dockerfile` → `pdo-sandbox` ; `Dockerfile.chrome-dev` → `pdo-sandbox-chrome-dev`. Le suffixe
/// est slugifié (minuscules, tout non-alphanumérique → `-`, runs collapsés, bords rognés) parce
/// qu'un nom de dépôt Docker doit matcher `[a-z0-9]+([._-]+[a-z0-9]+)*` : un nom de fichier
/// arbitraire ne peut pas être recopié tel quel dans un tag.
///
/// Tout autre nom de fichier (`sbx.Dockerfile`, `Dockerfile-custom`, …) retombe sur
/// [`BASE_IMAGE_NAME`] : la variante est une donnée du fichier LIVRÉ par PDO, pas une convention
/// imposée aux Dockerfiles qu'un profil pointe (#431, #467) — le tag reste
/// de toute façon le hash de SES octets, donc deux variantes distinctes ne peuvent pas collisionner
/// sous le même nom.
///
/// C'est le SEUL endroit qui connaît la règle ; `release.yml` la re-dérive en bash avec un
/// self-check de parité, miroir de celui de [`dockerfile_hash`].
pub fn image_name_for_dockerfile(path: &Path) -> String {
    let Some(suffix) = path
        .file_name()
        .and_then(|n| n.to_str())
        .and_then(|n| n.strip_prefix("Dockerfile."))
    else {
        return BASE_IMAGE_NAME.to_string();
    };
    let mut slug = String::with_capacity(suffix.len());
    for c in suffix.chars() {
        if c.is_ascii_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        BASE_IMAGE_NAME.to_string()
    } else {
        format!("{BASE_IMAGE_NAME}-{slug}")
    }
}

/// Ref locale `<image_name>:h-<hash>` (p.ex. `pdo-sandbox:h-<hash>`). Le NOM est un paramètre
/// depuis #466 (variantes d'image) ; le hash, lui, ne dépend que des octets du Dockerfile.
/// (GHCR #411 formate son propre préfixe autour du même couple nom+hash.)
pub fn local_image_ref(image_name: &str, dockerfile_bytes: &[u8]) -> String {
    format!("{image_name}:h-{}", dockerfile_hash(dockerfile_bytes))
}

/// Préfixe GHCR des images publiées (#411, paramétré par #466). Owner lowercasé (GHCR rejette
/// l'uppercase). MÊME hash que [`local_image_ref`] → pull et build local interchangeables sous le
/// même contenu (ADR-0030 pt 7). `release.yml` construit ce même chemin en bash
/// (`${GITHUB_REPOSITORY_OWNER,,}`).
pub const REGISTRY_PREFIX: &str = "ghcr.io/loulen";

/// Ref registry `ghcr.io/loulen/<image_name>:h-<hash>` (MÊME nom+hash que [`local_image_ref`], donc
/// pull et build sont interchangeables sous le ref local après retag).
pub fn registry_image_ref(image_name: &str, dockerfile_bytes: &[u8]) -> String {
    format!(
        "{REGISTRY_PREFIX}/{image_name}:h-{}",
        dockerfile_hash(dockerfile_bytes)
    )
}

/// D'où [`crate::application::image::ensure_image`] tire l'image hash-dérivée (#411). **Par-daemon**, PAS par-Run :
/// contrairement à [`crate::domain::mode::SandboxMode`], NE PAS la porter sur `RunStarted`.
///
/// Ni `Default` ni `DEFAULT` ici depuis #471 : le défaut est une donnée de la **couche de défauts
/// de profil** ([`crate::domain::profile::DEFAULT_PROFILE_IMAGE`]), et un `#[default]` dupliqué ici
/// serait un second propriétaire du même fait (discipline #447), donc une dérive en attente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSource {
    /// Pull `ghcr.io/loulen/pdo-sandbox:h-<hash>`, retag local, build en fallback.
    Registry,
    /// Ne jamais tirer : build local depuis le Dockerfile résolu (comportement #405).
    Dockerfile,
}

impl ImageSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ImageSource::Registry => "registry",
            ImageSource::Dockerfile => "dockerfile",
        }
    }

    /// Parse la forme filaire ; `None` pour tout token inconnu (le résolveur les traite comme
    /// unset). Miroir de `ServiceHealthOverride::parse`. Depuis #471 son seul appelant est le
    /// tier env : il ne passe par aucun validateur, donc un token bidon dans l'env du daemon
    /// retombe silencieusement sur le défaut de profil — le contraire d'un `panic!` au boot.
    pub fn parse(s: &str) -> Option<ImageSource> {
        match s.trim().to_ascii_lowercase().as_str() {
            "registry" => Some(ImageSource::Registry),
            "dockerfile" => Some(ImageSource::Dockerfile),
            _ => None,
        }
    }
}

// -- #467 : la source d'image appartient (aussi) au profil de staging ---------

/// La source d'image que porte un **profil de staging** (#467), telle qu'elle est stockée
/// (colonne JSON `sandbox_profiles.image`), gelée (clé `sandbox_image` de `RunStarted`) et servie
/// (vue de l'éditeur de profil). `None` côté appelant = « ce profil ne pose rien » → le défaut de
/// profil décide ([`crate::domain::profile::DEFAULT_PROFILE_IMAGE`], overridable par l'env).
///
/// Les deux bras sont **interchangeables** dans le formulaire, mais radicalement différents en
/// aval : voir [`ImagePlan`]. Le tag interne (`kind`) est la forme filaire, et elle est
/// permanente — un profil sérialisé aujourd'hui doit se relire dans dix versions.
///
/// Défini ici et non dans [`crate::domain::profile`] : c'est du vocabulaire d'**image**, dont ce
/// module est le propriétaire (le hash, les refs, `ensure_image`). Le profil le stocke et le
/// valide, comme il valide des clés d'env dont [`crate::domain::container`] possède la liste
/// réservée (discipline #447 : un fait, un propriétaire). Le Run la gèle dans son état
/// (`RunState::sandbox_image`), comme [`crate::domain::mode::SandboxMode`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "SandboxProfileImage")]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileImage {
    /// Un Dockerfile choisi PAR PROFIL : la même mécanique que #431, au tier
    /// [`DockerfileSource::Profile`]. Tout le reste est inchangé — le tag reste le hash de
    /// ses octets, le nom vient de son nom de fichier (#466), le contexte de build reste vide,
    /// donc **il doit être auto-porteur** (pas de `COPY`).
    Dockerfile { path: String },
    /// Un ref registry **explicite**, tiré tel quel. Casse l'adressage par contenu, et c'est
    /// assumé : voir [`crate::application::image::ensure_explicit_ref`] et l'amendement #467 d'ADR-0030 pt 7.
    Registry {
        #[serde(rename = "ref")]
        image_ref: String,
    },
}

impl ProfileImage {
    /// La forme filaire du bras, pour les messages d'erreur et l'UI.
    pub fn kind_str(&self) -> &'static str {
        match self {
            ProfileImage::Dockerfile { .. } => "dockerfile",
            ProfileImage::Registry { .. } => "registry",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le Dockerfile de base LIVRÉ, celui que l'adaptateur seede.
    const EMBEDDED_DOCKERFILE: &str = include_str!("../../../assets/sandbox/Dockerfile");

    /// Le Dockerfile de variante LIVRÉ, lu au build du test uniquement (`#[cfg(test)]`) : il n'a
    /// pas à grossir le binaire, personne ne le seede — c'est un profil (ou l'env) qui le pointe.
    const EMBEDDED_CHROME_DEV_DOCKERFILE: &str =
        include_str!("../../../assets/sandbox/Dockerfile.chrome-dev");

    #[test]
    fn dockerfile_tag_stable_and_edit_sensitive() {
        let base: &[u8] = b"FROM ubuntu:24.04\nRUN apt-get update\n";
        assert_eq!(
            local_image_ref(BASE_IMAGE_NAME, base),
            local_image_ref(BASE_IMAGE_NAME, base)
        );
        let edited: &[u8] = b"FROM ubuntu:24.04\nRUN apt-get update\nRUN apt-get install -y git\n";
        assert_ne!(dockerfile_hash(base), dockerfile_hash(edited));

        let h = dockerfile_hash(base);
        assert_eq!(h.len(), 12);
        assert!(h
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(local_image_ref(BASE_IMAGE_NAME, base).starts_with("pdo-sandbox:h-"));

        // GARDE-FOU PARITÉ CI : figer l'algo canonique. Épingle la sortie Rust au préfixe que
        // `release.yml`/#411 produiront en bash :
        //   printf 'FROM ubuntu:24.04\nRUN apt-get update\n' | sha256sum | cut -c1-12
        assert_eq!(h, "5804eefb8f92");
    }

    #[test]
    fn image_name_for_dockerfile_derives_the_variant_from_the_filename() {
        // La base garde le nom historique — sinon toute instance existante rebuild pour rien.
        assert_eq!(
            image_name_for_dockerfile(Path::new("/home/u/.pdo/sandbox/Dockerfile")),
            "pdo-sandbox"
        );
        // La variante livrée : c'est CE couple que `release.yml` publie.
        assert_eq!(
            image_name_for_dockerfile(Path::new(
                "/repo/crates/pdo-sandbox/assets/sandbox/Dockerfile.chrome-dev"
            )),
            "pdo-sandbox-chrome-dev"
        );
        // Slugification : minuscules, non-alphanumériques → `-`, runs collapsés, bords rognés —
        // un nom de dépôt Docker doit matcher `[a-z0-9]+([._-]+[a-z0-9]+)*`.
        assert_eq!(
            image_name_for_dockerfile(Path::new("Dockerfile.Chrome_Dev")),
            "pdo-sandbox-chrome-dev"
        );
        assert_eq!(
            image_name_for_dockerfile(Path::new("Dockerfile.a..b")),
            "pdo-sandbox-a-b"
        );
        assert_eq!(
            image_name_for_dockerfile(Path::new("Dockerfile.-x-")),
            "pdo-sandbox-x"
        );
        // Suffixe vide ou nom sans le préfixe `Dockerfile.` → la base. Un Dockerfile que
        // l'utilisateur POINTE (#431) n'a aucune raison de suivre notre convention de nommage ;
        // son tag reste le hash de SES octets, donc aucune collision possible.
        assert_eq!(
            image_name_for_dockerfile(Path::new("Dockerfile.")),
            "pdo-sandbox"
        );
        assert_eq!(
            image_name_for_dockerfile(Path::new("/repo/docker/sbx.Dockerfile")),
            "pdo-sandbox"
        );
        assert_eq!(
            image_name_for_dockerfile(Path::new("Dockerfile-custom")),
            "pdo-sandbox"
        );
    }

    #[test]
    fn registry_ref_is_namespaced_per_variant() {
        let bytes: &[u8] = b"FROM ubuntu:24.04\n";
        let h = dockerfile_hash(bytes);
        // La base garde son chemin GHCR historique, octet pour octet.
        assert_eq!(
            registry_image_ref(BASE_IMAGE_NAME, bytes),
            format!("ghcr.io/loulen/pdo-sandbox:h-{h}")
        );
        // La variante est un dépôt GHCR distinct sous le même owner (lowercase : GHCR rejette
        // l'uppercase) — c'est ce que la matrice de `release.yml` pousse.
        assert_eq!(
            registry_image_ref("pdo-sandbox-chrome-dev", bytes),
            format!("ghcr.io/loulen/pdo-sandbox-chrome-dev:h-{h}")
        );
        // MÊME hash des deux côtés du couple local/registry → pull et build interchangeables.
        assert!(registry_image_ref("pdo-sandbox-chrome-dev", bytes)
            .ends_with(&local_image_ref("pdo-sandbox-chrome-dev", bytes)));
    }

    #[test]
    fn shipped_chrome_dev_dockerfile_is_self_contained() {
        // Pin des DEUX décisions structurantes du fichier livré (#466), qu'une « simplification »
        // future casserait silencieusement :
        //   1. AUTONOME : `FROM ubuntu:24.04`, pas `FROM ghcr.io/loulen/pdo-sandbox:h-<hash>` —
        //      injecter le hash de la base demanderait de GÉNÉRER ces octets, or ces octets SONT
        //      la source de vérité du tag (ADR-0030 pt 7).
        //   2. Pas de `COPY`/`ADD` : le contexte de build est `<sandbox_root>/.build-ctx`, VIDE
        //      (D8) — un `COPY` échouerait au premier build, en prod, chez l'utilisateur.
        let df = EMBEDDED_CHROME_DEV_DOCKERFILE;
        let directives: Vec<&str> = df
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .collect();
        assert_eq!(
            directives.first().copied(),
            Some("FROM ubuntu:24.04"),
            "la variante doit être autonome, pas dérivée du tag de la base"
        );
        assert!(
            !directives.iter().any(|l| l.starts_with("FROM ghcr.io")),
            "un `FROM ghcr.io/...:h-<hash>` obligerait à générer ce fichier"
        );
        assert!(
            !directives
                .iter()
                .any(|l| l.starts_with("COPY ") || l.starts_with("ADD ")),
            "le contexte de build est vide (D8) : un COPY/ADD échouerait au premier build"
        );
        // Le nom d'image de la variante vient du nom de ce fichier, pas d'une constante à part.
        assert_eq!(
            image_name_for_dockerfile(Path::new("Dockerfile.chrome-dev")),
            "pdo-sandbox-chrome-dev"
        );
        // Le hash de la BASE ne dépend pas de ce fichier (AC 3) : deux fichiers, deux tags.
        assert_ne!(
            dockerfile_hash(df.as_bytes()),
            dockerfile_hash(EMBEDDED_DOCKERFILE.as_bytes())
        );
    }

    /// La forme filaire est PERMANENTE : un profil sérialisé aujourd'hui doit se relire dans dix
    /// versions. Le tag interne `kind`, et le nom de champ `ref` (mot-clé Rust, donc renommé côté
    /// serde) sont donc pinnés ici plutôt que laissés à la dérive du dérive.
    #[test]
    fn profile_image_wire_form_is_pinned() {
        let df = ProfileImage::Dockerfile {
            path: "/repo/docker/Dockerfile.chrome-dev".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&df).unwrap(),
            serde_json::json!({ "kind": "dockerfile", "path": "/repo/docker/Dockerfile.chrome-dev" })
        );
        let reg = ProfileImage::Registry {
            image_ref: "ghcr.io/acme/agent:1.4".to_string(),
        };
        assert_eq!(
            serde_json::to_value(&reg).unwrap(),
            serde_json::json!({ "kind": "registry", "ref": "ghcr.io/acme/agent:1.4" })
        );
        // Round-trip both ways, from the wire form a hand-written DB row would hold.
        assert_eq!(
            serde_json::from_value::<ProfileImage>(
                serde_json::json!({ "kind": "registry", "ref": "x:1" })
            )
            .unwrap(),
            ProfileImage::Registry {
                image_ref: "x:1".to_string()
            }
        );
        assert_eq!(df.kind_str(), "dockerfile");
        assert_eq!(reg.kind_str(), "registry");
        // An unknown kind does NOT silently become one of the two.
        assert!(serde_json::from_value::<ProfileImage>(
            serde_json::json!({ "kind": "ecr", "ref": "x:1" })
        )
        .is_err());
    }
}
