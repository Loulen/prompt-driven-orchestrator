//! Fourniture de l'image sandbox (#405 build local, slice B du PRD #403 ; #411 pull GHCR hybride).
//!
//! Miroir de [`crate::worktree_ops`] / [`crate::sandbox_staging`] : pas d'`AppState`,
//! pas d'async, pas de lecture d'env dans le cœur — `&Path`/`&str`/`&[u8]` in,
//! path-math ou `std::fs`/`std::process::Command` out. `HOME` et le binaire docker
//! ne sont lus QUE par les résolveurs de bord.
//!
//! Ce module garantit qu'une image `pdo-sandbox:h-<hash>` (tag = hash du CONTENU du
//! Dockerfile) existe localement. [`ensure_image`] est **hybride** (#411) : si l'image
//! n'est pas déjà locale et que la source est [`ImageSource::Registry`] (défaut), il
//! `docker pull ghcr.io/loulen/pdo-sandbox:h-<hash>` puis retague vers le ref local ;
//! un pull raté (offline / tag absent / registry down) retombe sur le build local, et
//! [`ImageSource::Dockerfile`] build directement sans jamais tirer. La valeur de retour
//! reste TOUJOURS le ref local `pdo-sandbox:h-<hash>` (le même [`dockerfile_hash`] côté
//! pull et build), donc [`crate::sandbox_container`] est inchangé. Les slices sœurs le
//! CONSOMMENT :
//! - #406 instancie un conteneur à partir de l'image ;
//! - #407 câble [`ensure_image`] dans le run-advance (ADR-0030) — via `spawn_blocking`
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
//! ([`ProfileImage`]), et [`ensure_image`] devient un aiguillage sur un [`ImagePlan`] à **deux**
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
//! ([`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`], à côté de `DEFAULT_FULL_ENTRIES`) —
//! registre hash-dérivé sur le Dockerfile seedé, exactement ce que le tier `default` des deux
//! réglages produisait. Les deux tiers ENV ([`IMAGE_SOURCE_ENV`], [`DOCKERFILE_PATH_ENV`]) sont
//! CONSERVÉS et repointés sur ce défaut : une instance headless n'a que des profils virtuels et
//! pas d'UI, donc l'env est son seul moyen de changer d'image sans POSTer un profil.

#![allow(dead_code)] // Tracer bullet : consommé par #406/#407, non câblé dans cette slice.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Dockerfile embarqué (source de vérité dans le binaire). Seedé sur disque au 1er usage.
/// `.gitattributes` épingle `eol=lf` sur ce fichier : `include_str!` embarque les octets
/// verbatim, donc un checkout CRLF changerait le hash (cf. D6 du plan #405).
const EMBEDDED_DOCKERFILE: &str = include_str!("../assets/sandbox/Dockerfile");

/// Env var pointant les invocations docker vers un exécutable fake (seam test/intégration).
/// Miroir de [`crate::tmux_session_manager::TMUX_CMD_OVERRIDE_ENV`] : lue UNE fois au bord,
/// jamais dans le cœur.
pub const DOCKER_CMD_OVERRIDE_ENV: &str = "PDO_DOCKER_CMD_OVERRIDE";

/// Message d'erreur unique quand le binaire `docker` est introuvable sur le PATH. Devient la
/// `reason` d'un `RunFailed` (US-16) : jamais d'exécution silencieuse sur l'hôte.
/// `pub(crate)` : réutilisé verbatim par [`crate::sandbox_container`] (#406) — un seul message
/// canonique docker-absent partagé par les deux modules sandbox.
pub(crate) const DOCKER_NOT_FOUND_MSG: &str =
    "sandbox run requires Docker, but the `docker` binary was not found on PATH — \
     install Docker or set this run's sandbox to `off`";

/// Message when the `docker` binary IS present but its daemon is unreachable (#410).
/// The precise reason the availability probe grays out `full`/`minimal` in the UI. This
/// case and `DOCKER_NOT_FOUND_MSG` both collapse to `available: false` — no action is
/// gated on the distinction (the probe is advisory; the run-advance fail-fast, ADR-0030
/// pt 4, stays the authoritative gate) — so the cause survives only in `reason`.
pub(crate) const DOCKER_DAEMON_UNREACHABLE_MSG: &str =
    "the `docker` binary is present but the Docker daemon is unreachable — \
     start Docker or set this run's sandbox to `off`";

/// Advisory availability of a usable Docker for sandboxed Runs (#410). `available`
/// gates nothing on its own (the run-advance fail-fast is authoritative); it drives
/// the NewRunModal's greying of `full`/`minimal`. `reason` carries the human-readable
/// cause when unavailable (one of the two messages above), `None` when available.
#[derive(Debug, Clone)]
pub(crate) struct DockerProbe {
    pub available: bool,
    pub reason: Option<String>,
}

/// Probe whether this host can launch a sandboxed Run, by forcing a round-trip to the
/// Docker daemon: `docker version --format '{{.Server.Version}}'` (exit 0 ⇔ the daemon
/// answered). Three signals collapse to two states: spawn `NotFound` → not installed
/// (`DOCKER_NOT_FOUND_MSG`); any other spawn error or a non-zero exit → daemon
/// unreachable (`DOCKER_DAEMON_UNREACHABLE_MSG`); exit 0 → available.
///
/// Sync `std::process`, mirror of the rest of this leaf module: `docker_bin` is
/// threaded in (never read from `std::env` here), so the caller owns the env/timeout
/// at the edge (`spawn_blocking` + `tokio::time::timeout`, never the runtime thread).
/// Rejected alternatives (see plan D2): "binary on PATH" alone misses a stopped daemon
/// — precisely the case to gray out; an `ensure_image` dry-run costs minutes + network.
pub(crate) fn probe_docker(docker_bin: &str) -> DockerProbe {
    match Command::new(docker_bin)
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
    {
        Ok(o) if o.status.success() => DockerProbe {
            available: true,
            reason: None,
        },
        Ok(_) => DockerProbe {
            available: false,
            reason: Some(DOCKER_DAEMON_UNREACHABLE_MSG.to_string()),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => DockerProbe {
            available: false,
            reason: Some(DOCKER_NOT_FOUND_MSG.to_string()),
        },
        Err(_) => DockerProbe {
            available: false,
            reason: Some(DOCKER_NOT_FOUND_MSG.to_string()),
        },
    }
}

// -- path math (pur, sans IO) ------------------------------------------------

/// `<sandbox_root>/Dockerfile` — emplacement canonique du Dockerfile **seedé**, et le
/// tier `default` de [`resolve_dockerfile`] (#431). Renommé depuis `dockerfile_path` :
/// depuis que le chemin est réglable, un `dockerfile_path` nu serait un nom menteur.
pub(crate) fn default_dockerfile_path(sandbox_root: &Path) -> PathBuf {
    sandbox_root.join("Dockerfile")
}

/// `<sandbox_root>/.build-ctx` — contexte de build dédié, gardé VIDE (cf. D8) : `~/.pdo/sandbox/`
/// a pour siblings les staging dirs par-run (`<run-id>/claude-home/`, ~98 Mo, écrits
/// concurremment) — l'utiliser comme contexte enverrait un tarball géant et racerait un run.
pub(crate) fn build_context_dir(sandbox_root: &Path) -> PathBuf {
    sandbox_root.join(".build-ctx")
}

// -- hash / tag (pur ; SINGLE SOURCE OF TRUTH pour #411 + release.yml) --------

/// SHA-256 sur les octets EXACTS du Dockerfile fed à `docker build`, 12 hex minuscules.
/// Équivalent CI CANONIQUE : `sha256sum Dockerfile | cut -c1-12`. Hasher les octets bruts ;
/// **jamais normaliser** (pas de conversion `\r\n`, pas de fix de newline finale) — le
/// hash-input DOIT == build-input, sinon réutilisation d'une image périmée (#411 hashe en bash).
pub(crate) fn dockerfile_hash(dockerfile_bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(dockerfile_bytes);
    let full = format!("{:x}", hasher.finalize());
    full[..12].to_string()
}

/// Nom d'image de la variante de BASE (`assets/sandbox/Dockerfile`).
pub(crate) const BASE_IMAGE_NAME: &str = "pdo-sandbox";

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
pub(crate) fn image_name_for_dockerfile(path: &Path) -> String {
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
pub(crate) fn local_image_ref(image_name: &str, dockerfile_bytes: &[u8]) -> String {
    format!("{image_name}:h-{}", dockerfile_hash(dockerfile_bytes))
}

/// Préfixe GHCR des images publiées (#411, paramétré par #466). Owner lowercasé (GHCR rejette
/// l'uppercase). MÊME hash que [`local_image_ref`] → pull et build local interchangeables sous le
/// même contenu (ADR-0030 pt 7). `release.yml` construit ce même chemin en bash
/// (`${GITHUB_REPOSITORY_OWNER,,}`).
pub(crate) const REGISTRY_PREFIX: &str = "ghcr.io/loulen";

/// Ref registry `ghcr.io/loulen/<image_name>:h-<hash>` (MÊME nom+hash que [`local_image_ref`], donc
/// pull et build sont interchangeables sous le ref local après retag).
pub(crate) fn registry_image_ref(image_name: &str, dockerfile_bytes: &[u8]) -> String {
    format!(
        "{REGISTRY_PREFIX}/{image_name}:h-{}",
        dockerfile_hash(dockerfile_bytes)
    )
}

// -- effets fs (sync std::fs, anyhow + .context) -----------------------------

/// Écrit le Dockerfile `embedded` à son chemin **par défaut** si absent ; sinon **ne touche
/// à rien** (édition utilisateur préservée : une édition change le hash donc rebuild). Renvoie
/// le chemin dans les deux cas.
///
/// #431 : n'écrit **JAMAIS** ailleurs qu'à [`default_dockerfile_path`], même quand un tier
/// `stored`/`env` a désigné un autre chemin — écrire `EMBEDDED` dans un emplacement du repo
/// que l'utilisateur a simplement *pointé* serait une mutation non demandée de son repo.
pub(crate) fn seed_dockerfile(sandbox_root: &Path, embedded: &str) -> Result<PathBuf> {
    let path = default_dockerfile_path(sandbox_root);
    if path.exists() {
        return Ok(path); // édition utilisateur gagne
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to seed sandbox Dockerfile at {}", path.display()))?;
    }
    std::fs::write(&path, embedded.as_bytes())
        .with_context(|| format!("failed to seed sandbox Dockerfile at {}", path.display()))?;
    Ok(path)
}

// -- effets docker (sync std::process::Command) ------------------------------

/// `docker image inspect <tag>` (métadonnée locale, jamais de réseau) : `Ok(true)` si exit 0
/// (présente), `Ok(false)` si exit != 0 (absente). `docker` introuvable (spawn `NotFound`) ->
/// `Err` explicite préservant l'`io::Error` en source (chaîne à 2 maillons, cf. #298).
pub(crate) fn image_exists(docker_bin: &str, tag: &str) -> Result<bool> {
    match Command::new(docker_bin)
        .args(["image", "inspect", tag])
        .output()
    {
        Ok(output) => Ok(output.status.success()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG)
        }
        Err(e) => {
            Err(e).context("failed to run `docker image inspect` while probing the sandbox image")
        }
    }
}

/// `docker build -t <tag> -f <dockerfile> <context_dir>` ; bail non-zéro avec le stderr docker
/// (chaque erreur est la `reason` actionnable d'un `RunFailed`, US-16).
pub(crate) fn build_image(
    docker_bin: &str,
    tag: &str,
    dockerfile: &Path,
    context_dir: &Path,
) -> Result<()> {
    let output = match Command::new(docker_bin)
        .arg("build")
        .arg("-t")
        .arg(tag)
        .arg("-f")
        .arg(dockerfile)
        .arg(context_dir)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker build` while building the sandbox image");
        }
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to build the sandbox image `{tag}` from {} — \
             `docker build` exited with {}: {stderr}",
            dockerfile.display(),
            output.status
        );
    }
    Ok(())
}

/// Issue d'un `docker pull`. `ok` est le SEUL verdict (l'exit code) ; `stderr` n'existe que pour
/// être **rapporté** quand l'échec est une erreur dure (#467 : un ref explicite n'a pas de repli
/// build, donc la raison du 404/auth/offline est la seule chose actionnable qui reste, US-16).
/// Ne jamais dériver le verdict du stderr : `docker pull` y écrit sa PROGRESSION en cas de succès.
#[derive(Debug, Clone)]
pub(crate) struct PullOutcome {
    pub(crate) ok: bool,
    pub(crate) stderr: String,
}

/// `docker pull <registry_ref>` (réseau, image PUBLIQUE, sans auth) : `ok: true` si exit 0 (tirée),
/// `ok: false` si exit != 0 (offline / 404 tag absent / registry down → fallback build en
/// hash-dérivé, erreur dure sur un ref explicite). `docker` introuvable (spawn `NotFound`) → `Err`
/// explicite (jamais de fallback silencieux masquant Docker absent — miroir strict
/// d'[`image_exists`]). Le stderr de PROGRESSION de `docker pull` n'est PAS un signal d'échec :
/// seul l'exit code compte.
pub(crate) fn pull_image(docker_bin: &str, registry_ref: &str) -> Result<PullOutcome> {
    match Command::new(docker_bin)
        .args(["pull", registry_ref])
        .output()
    {
        Ok(output) => Ok(PullOutcome {
            ok: output.status.success(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG)
        }
        Err(e) => Err(e).context("failed to run `docker pull` while fetching the sandbox image"),
    }
}

/// `docker tag <src> <dst>` : retague l'image tirée sous le ref local content-addressé, pour que
/// [`crate::sandbox_container`] reçoive TOUJOURS `pdo-sandbox:h-<hash>` (pull ou build → même nom).
/// Idempotent. Non-zéro → bail avec stderr (reason actionnable d'un `RunFailed`, US-16), miroir de
/// [`build_image`]. `docker` introuvable → `Err` explicite (comme [`pull_image`]).
pub(crate) fn tag_image(docker_bin: &str, src: &str, dst: &str) -> Result<()> {
    let output = match Command::new(docker_bin).args(["tag", src, dst]).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => return Err(e).context("failed to run `docker tag` for the sandbox image"),
    };
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to retag the pulled sandbox image `{src}` as `{dst}` — \
             `docker tag` exited with {}: {stderr}",
            output.status
        );
    }
    Ok(())
}

/// Provisionneur idempotent (**seul point d'entrée** de #406/#407) : garantit que l'image du Run
/// existe localement et retourne le ref que `sandbox_container` doit poser au `docker create`.
///
/// Aiguillage à deux branches sur le plan résolu au bord (#467) — voir [`ImagePlan`] :
/// [`ensure_hash_derived_image`] (le chemin historique, adressé par contenu) ou
/// [`ensure_explicit_ref`] (un ref registry libre posé par un profil : pull-ou-échec, jamais de
/// build). C'est ici, et nulle part ailleurs, que la différence se décide.
///
/// **Sync délibéré (D3)** : `docker build`/`docker pull` sont bloquants et longs, leur place est
/// dans le `spawn_blocking` du caller async (#407), pas dans une tâche tokio ; garder ce module
/// sync laisse aussi les tests en `#[test]` simples.
pub(crate) fn ensure_image(
    docker_bin: &str,
    sandbox_root: &Path,
    plan: &ImagePlan,
) -> Result<String> {
    match plan {
        ImagePlan::HashDerived { dockerfile, source } => {
            ensure_hash_derived_image(docker_bin, sandbox_root, dockerfile, *source)
        }
        ImagePlan::ExplicitRef { image_ref, profile } => {
            ensure_explicit_ref(docker_bin, image_ref, profile)
        }
    }
}

/// Garantit qu'un ref registry **explicite** (#467) est présent localement, et le retourne **tel
/// quel**. Deux invocations docker au plus, dans cet ordre :
///
/// 1. `image inspect <ref>` — fast-path zéro réseau : une image déjà locale (tirée hier, ou buildée
///    à la main par l'utilisateur sous ce nom) est réutilisée offline ;
/// 2. `docker pull <ref>` — et son échec est une **erreur DURE nommant le ref**.
///
/// Le contraste avec [`ensure_hash_derived_image`] est le cœur de l'amendement #467 d'ADR-0030
/// pt 7, et il n'est pas une préférence de style :
/// - **pas de repli build.** Le repli existe parce que `pdo-sandbox:h-<hash>` est le hash des
///   octets d'un Dockerfile *connu* : tirer ou builder produit alors la même image. Un ref libre
///   n'a pas de Dockerfile, donc pas de hash, donc rien à builder — un « fallback » ne pourrait que
///   builder une image SANS RAPPORT et la faire passer pour celle demandée ;
/// - **pas de retag.** Le retag existe pour ramener une image tirée sous le ref local
///   content-addressé ; ici le ref demandé EST le ref local, il n'y a pas de second nom ;
/// - **pas de seed de Dockerfile, aucune IO** : cette branche ne touche pas le disque.
///
/// PDO ne vérifie pas que l'image contient `claude` : c'est la responsabilité de qui fournit le
/// ref (ADR-0030 pt 7, amendement #467). Une image sans `claude` échouera au premier `docker exec`,
/// avec le stderr de docker — pas ici.
pub(crate) fn ensure_explicit_ref(
    docker_bin: &str,
    image_ref: &str,
    profile: &str,
) -> Result<String> {
    if image_exists(docker_bin, image_ref)? {
        return Ok(image_ref.to_string());
    }
    let pull = pull_image(docker_bin, image_ref)?;
    if !pull.ok {
        let stderr = pull.stderr.trim();
        anyhow::bail!(
            "failed to pull the sandbox image `{image_ref}` named by the staging profile \
             `{profile}` — `docker pull` failed{}. An explicit registry ref has no Dockerfile, \
             hence no content hash, hence NO local build to fall back to (ADR-0030 pt 7): fix the \
             ref, make it reachable, or point the profile at a Dockerfile instead.",
            if stderr.is_empty() {
                String::new()
            } else {
                format!(": {stderr}")
            }
        );
    }
    Ok(image_ref.to_string())
}

/// Provisionneur **hybride adressé par contenu** : garantit que le ref local
/// `<nom>:h-<hash>` existe et le retourne TOUJOURS (invariant `sandbox_container`).
///
/// Ordre (D7) : seed → **contrôle `is_file()` du chemin résolu** → lit octets → `local_ref` →
/// **fast-path `image_exists(local_ref)` (zéro réseau, offline-safe)** → si
/// [`ImageSource::Registry`] **et** emplacement par défaut : `docker pull` le `registry_ref`, OK →
/// `docker tag` vers `local_ref` → retour ; pull raté → fallthrough vers le build local → retour ;
/// build KO → `Err`. [`ImageSource::Dockerfile`] : build direct, **jamais** de pull.
pub(crate) fn ensure_hash_derived_image(
    docker_bin: &str,
    sandbox_root: &Path,
    dockerfile: &ResolvedDockerfile,
    source: ImageSource,
) -> Result<String> {
    // 1. Seed le Dockerfile PAR DÉFAUT si absent — TOUJOURS, même sous un chemin custom :
    //    c'est la copie de référence que l'utilisateur édite et la matérialisation du tier
    //    `default` pour quand le réglage est effacé. JAMAIS écrit à un chemin custom.
    seed_dockerfile(sandbox_root, EMBEDDED_DOCKERFILE)?;
    // 2. Le Dockerfile RÉSOLU (#431). Chemin absent / non-fichier-régulier = erreur DURE
    //    nommant chemin + tier : jamais de repli silencieux vers le seedé (ADR-0030 pt 4,
    //    amendement #431). `is_file()` et NON `exists()` — `exists()` est VRAI pour un
    //    répertoire, et `is_file()` suit correctement un Dockerfile symlinké tout en
    //    rejetant un lien pendant. Ce contrôle DOIT précéder le fast-path `image_exists` :
    //    le tag EST le hash de ces octets, donc sans eux l'image est innommable.
    let path = dockerfile.path.as_path();
    if !path.is_file() {
        // La remédiation dépend du tier gagnant : dire « édite le profil » à qui a posé
        // `PDO_SANDBOX_DOCKERFILE` dans l'env du daemon l'enverrait au mauvais endroit, et
        // réciproquement. Le tier et le chemin, eux, sont nommés dans les trois cas. Le tier
        // `default` ne peut pointer que le Dockerfile seedé, que `seed_dockerfile` vient
        // d'écrire quelques lignes plus haut : y arriver signifie que quelque chose l'a
        // supprimé entre-temps (#471 — plus aucun réglage stocké ne peut pointer ailleurs).
        let seeded = default_dockerfile_path(sandbox_root).display().to_string();
        let fix = match dockerfile.source {
            DockerfileSource::Profile => "point the staging profile's image at an existing \
                 Dockerfile, or set the profile back to the default image"
                .to_string(),
            DockerfileSource::Env => format!(
                "fix `{DOCKERFILE_PATH_ENV}` in the daemon's environment, or unset it to fall \
                 back to the seeded default at {seeded}"
            ),
            DockerfileSource::Default => format!(
                "the seeded Dockerfile at {seeded} vanished under the daemon; restart it to \
                 re-seed the reference copy"
            ),
        };
        anyhow::bail!(
            "the sandbox Dockerfile resolved from the `{}` tier does not exist or is not a \
             regular file: {} — {fix}",
            dockerfile.source.as_str(),
            path.display(),
        );
    }
    // 3. Octets bruts sur disque = entrée EXACTE du hash ET du build (jamais normaliser).
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read sandbox Dockerfile at {}", path.display()))?;
    // 4. Ref local content-addressé = TOUJOURS la valeur de retour (invariant sandbox_container).
    //    Le NOM vient du fichier résolu (#466 : `Dockerfile.chrome-dev` → `pdo-sandbox-chrome-dev`),
    //    le TAG de ses octets — deux variantes ne partagent donc ni nom ni tag.
    let local_ref = local_image_ref(&dockerfile.image_name, &bytes);
    // 5. FAST PATH — précède TOUT réseau : image déjà locale → ni pull ni build (offline-safe).
    if image_exists(docker_bin, &local_ref)? {
        return Ok(local_ref);
    }
    // 6. Registry : PULL avant build, mais SEULEMENT pour l'emplacement seedé par défaut —
    //    `release.yml` publie `h-<hash>` du Dockerfile d'un arbre de release, donc le hash
    //    d'un Dockerfile custom ne peut pas exister en amont (ADR-0030 §5, précisé par
    //    #431 : le prédicat porte sur le CHEMIN, pas sur les octets — comparer les octets
    //    à l'embarqué classerait « custom » toute machine ayant mis PDO à jour, dont le
    //    tag EXISTE sur GHCR, et lui imposerait un build local de plusieurs minutes).
    //    OK → retag sous le ref local → retour. Échec (offline / 404 / registry down) →
    //    fallthrough vers le build local ci-dessous.
    if matches!(source, ImageSource::Registry) && dockerfile.is_default_location {
        let registry_ref = registry_image_ref(&dockerfile.image_name, &bytes);
        if pull_image(docker_bin, &registry_ref)?.ok {
            tag_image(docker_bin, &registry_ref, &local_ref)?;
            return Ok(local_ref);
        }
    }
    // 7. Build local (mode dockerfile, chemin custom, OU fallback d'un pull raté). Contexte dédié
    //    VIDE (D8 : jamais sandbox_root, siblings = staging par-run) — inconditionnellement, y
    //    compris sous un chemin custom, donc **un Dockerfile pointé doit être auto-porteur (pas de
    //    `COPY`/`ADD`)** : suivre le parent du fichier pointé réouvrirait D8 et ferait du tag
    //    adressé par contenu un mensonge que le fast-path figerait pour toujours (ADR-0030 §5,
    //    amendement #431). Un `COPY` contre un contexte vide échoue bruyamment et `build_image`
    //    bail avec ce stderr verbatim. v1: double-build concurrent premier-run accepté (deux
    //    `docker build -t <même tag>` sont sûrs — daemon sérialise + cache, la sonde court-circuite
    //    le 2e run) ; ajouter un lock par tag si ça mord.
    let context_dir = build_context_dir(sandbox_root);
    std::fs::create_dir_all(&context_dir).with_context(|| {
        format!(
            "failed to create sandbox build context at {}",
            context_dir.display()
        )
    })?;
    build_image(docker_bin, &local_ref, path, &context_dir)?;
    Ok(local_ref)
}

// -- résolveurs de bord (seuls lecteurs d'env) -------------------------------

/// Binaire docker : [`DOCKER_CMD_OVERRIDE_ENV`] sinon `"docker"`.
pub(crate) fn docker_bin_from_env() -> String {
    std::env::var(DOCKER_CMD_OVERRIDE_ENV).unwrap_or_else(|_| "docker".to_string())
}

/// Racine de staging par défaut `~/.pdo/sandbox` depuis `HOME` (un seul `PathBuf` : l'image est
/// par-daemon, pas par-run). Miroir de [`crate::sandbox_staging::default_roots_from_env`].
pub(crate) fn default_sandbox_root_from_env() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    Some(home.join(".pdo").join("sandbox"))
}

/// D'où [`ensure_image`] tire l'image hash-dérivée (#411). **Par-daemon**, PAS par-Run :
/// contrairement à [`crate::event_log::SandboxMode`], NE PAS la porter sur `RunStarted`.
///
/// Ni `Default` ni `DEFAULT` ici depuis #471 : le défaut est une donnée de la **couche de défauts
/// de profil** ([`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`]), et un `#[default]` dupliqué ici
/// serait un second propriétaire du même fait (discipline #447), donc une dérive en attente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageSource {
    /// Pull `ghcr.io/loulen/pdo-sandbox:h-<hash>`, retag local, build en fallback.
    Registry,
    /// Ne jamais tirer : build local depuis le Dockerfile résolu (comportement #405).
    Dockerfile,
}

impl ImageSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ImageSource::Registry => "registry",
            ImageSource::Dockerfile => "dockerfile",
        }
    }

    /// Parse la forme filaire ; `None` pour tout token inconnu (le résolveur les traite comme
    /// unset). Miroir de `ServiceHealthOverride::parse`. Depuis #471 son seul appelant est le
    /// tier env : il ne passe par aucun validateur, donc un token bidon dans l'env du daemon
    /// retombe silencieusement sur le défaut de profil — le contraire d'un `panic!` au boot.
    pub(crate) fn parse(s: &str) -> Option<ImageSource> {
        match s.trim().to_ascii_lowercase().as_str() {
            "registry" => Some(ImageSource::Registry),
            "dockerfile" => Some(ImageSource::Dockerfile),
            _ => None,
        }
    }
}

/// Env var overridant la source d'image du **défaut de profil** (tier optionnel). Lue UNE fois au
/// bord, jamais dans le cœur — miroir de [`DOCKER_CMD_OVERRIDE_ENV`].
///
/// CONSERVÉE par #471 alors que le réglage d'instance disparaît, et ce n'est pas une exception à
/// « on ne garde pas de champ mort » : une instance headless fraîche n'a que des profils virtuels
/// et pas d'UI, donc c'est son SEUL moyen de changer d'image sans POSTer un profil.
pub(crate) const IMAGE_SOURCE_ENV: &str = "PDO_SANDBOX_IMAGE_SOURCE";

/// Résolution PURE de la source d'image, `env → défaut de profil` (#471) — testable sans toucher
/// `std::env`, miroir de [`resolve_dockerfile`]. Le défaut n'est pas nommé ici : il appartient à
/// [`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`], comme la liste d'entrées de `full`.
pub(crate) fn resolve_image_source(env: Option<ImageSource>) -> ImageSource {
    env.unwrap_or(crate::sandbox_profile::DEFAULT_PROFILE_IMAGE.source)
}

/// Le tier env de la source d'image : `Some` si [`IMAGE_SOURCE_ENV`] porte un token connu, `None`
/// s'il est absent, vide, ou inconnu. Lu UNE fois au bord par [`image_plan_with`].
pub(crate) fn env_image_source() -> Option<ImageSource> {
    std::env::var(IMAGE_SOURCE_ENV)
        .ok()
        .as_deref()
        .and_then(ImageSource::parse)
}

// -- Dockerfile résolu : 3 tiers depuis #471 (profil → env → défaut) ---------

/// Quel tier a choisi le Dockerfile (#431, amputé du tier `stored` par #471). Miroir de la
/// discipline `as_str` d'[`ImageSource`] ; surfacé par la `reason` du `RunFailed` — qui regarde
/// un « no such file » a besoin de savoir QUI l'a dit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DockerfileSource {
    /// Le profil de staging du Run (#467) — le tier le plus fort, et le seul qui soit **par
    /// Run** plutôt que par daemon : il est gelé dans `RunStarted`, pas relu à chaque prep.
    Profile,
    /// [`DOCKERFILE_PATH_ENV`] dans l'env du daemon (#471 : l'échappatoire headless).
    Env,
    /// Le Dockerfile seedé, matérialisation de [`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`].
    Default,
}

impl DockerfileSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            DockerfileSource::Profile => "profile",
            DockerfileSource::Env => "env",
            DockerfileSource::Default => "default",
        }
    }
}

// -- #467 : la source d'image appartient (aussi) au profil de staging ---------

/// La source d'image que porte un **profil de staging** (#467), telle qu'elle est stockée
/// (colonne JSON `sandbox_profiles.image`), gelée (clé `sandbox_image` de `RunStarted`) et servie
/// (vue de l'éditeur de profil). `None` côté appelant = « ce profil ne pose rien » → le défaut de
/// profil décide ([`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`], overridable par l'env).
///
/// Les deux bras sont **interchangeables** dans le formulaire, mais radicalement différents en
/// aval : voir [`ImagePlan`]. Le tag interne (`kind`) est la forme filaire, et elle est
/// permanente — un profil sérialisé aujourd'hui doit se relire dans dix versions.
///
/// Défini ici et non dans [`crate::sandbox_profile`] : c'est du vocabulaire d'**image**, dont ce
/// module est le propriétaire (le hash, les refs, `ensure_image`). Le profil le stocke et le
/// valide, comme il valide des clés d'env dont [`crate::sandbox_container`] possède la liste
/// réservée (discipline #447 : un fait, un propriétaire).
/// `pub` and not `pub(crate)`, unlike everything else in this module, for exactly the reason
/// [`crate::event_log::SandboxMode`] is: it is a field of `RunState`, which some public signature
/// makes reachable — a `pub(crate)` type there is a `private_interfaces` warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProfileImage {
    /// Un Dockerfile choisi PAR PROFIL : la même mécanique que #431, au tier
    /// [`DockerfileSource::Profile`]. Tout le reste est inchangé — le tag reste le hash de
    /// ses octets, le nom vient de son nom de fichier (#466), le contexte de build reste vide,
    /// donc **il doit être auto-porteur** (pas de `COPY`).
    Dockerfile { path: String },
    /// Un ref registry **explicite**, tiré tel quel. Casse l'adressage par contenu, et c'est
    /// assumé : voir [`ensure_explicit_ref`] et l'amendement #467 d'ADR-0030 pt 7.
    Registry {
        #[serde(rename = "ref")]
        image_ref: String,
    },
}

impl ProfileImage {
    /// La forme filaire du bras, pour les messages d'erreur et l'UI.
    pub(crate) fn kind_str(&self) -> &'static str {
        match self {
            ProfileImage::Dockerfile { .. } => "dockerfile",
            ProfileImage::Registry { .. } => "registry",
        }
    }
}

/// Ce que [`ensure_image`] doit FAIRE pour un Run donné, résolu une fois au bord. Deux branches,
/// et l'intérêt de l'enum est justement qu'elles ne se mélangent pas : la première est adressée par
/// contenu (pull et build interchangeables), la seconde ne l'est pas (pull ou rien).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ImagePlan {
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
    pub(crate) fn known_ref(&self) -> Option<&str> {
        match self {
            ImagePlan::HashDerived { .. } => None,
            ImagePlan::ExplicitRef { image_ref, .. } => Some(image_ref),
        }
    }
}

/// Le plan d'image d'un Run, précédence **profil (si posé) → env → défaut de profil**
/// (#467, amputé du tier `stored` par #471). **PUR** : les deux tiers env sont *injectés*, pas lus
/// — c'est [`image_plan_with`] qui les lit, une fois, au bord.
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
pub(crate) fn resolve_image_plan(
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

/// Wrapper de bord de [`resolve_image_plan`] : lit les DEUX variables d'env une fois chacune, puis
/// délègue. SOURCE UNIQUE consommée par `sandbox_run::context_from_state`, donc elle voit
/// exactement les mêmes tiers que ce que `ensure_image` consommera (0 drift, leçon #373).
pub(crate) fn image_plan_with(
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

/// Le Dockerfile que [`ensure_image`] hashe et builde, résolu UNE fois au bord (#431).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDockerfile {
    pub(crate) path: PathBuf,
    pub(crate) source: DockerfileSource,
    /// Prédicat de skip-pull (ADR-0030 §5, précisé par #431) : porte sur l'EMPLACEMENT par
    /// défaut, **pas** sur le tier — un profil peut épingler le chemin seedé via le picker, et ça
    /// doit continuer à puller. Égalité `PathBuf` volontairement nue :
    /// `canonicalize` est de l'IO, échoue sur un chemin absent, et empoisonnerait la pureté du
    /// résolveur. Mal classer est inoffensif dans les deux sens (un 404 gâché, ou un pull évité
    /// qui aurait 404 de toute façon) — le skip-pull est une optimisation, pas un gate de
    /// correction.
    pub(crate) is_default_location: bool,
    /// Nom d'image que ce Dockerfile produit (#466), dérivé de son NOM DE FICHIER par
    /// [`image_name_for_dockerfile`] — `pdo-sandbox` pour la base, `pdo-sandbox-chrome-dev` pour
    /// `Dockerfile.chrome-dev`. Résolu ici, avec le chemin, pour que `ensure_image` **et** la
    /// disclosure `GET /settings` nomment la même image (0 drift, leçon #373).
    pub(crate) image_name: String,
}

/// Env var pointant le Dockerfile du **défaut de profil** (tier optionnel, #431). Lue UNE fois au
/// bord, jamais dans le cœur — miroir de [`IMAGE_SOURCE_ENV`], et CONSERVÉE par #471 pour la même
/// raison headless. Ne passe par aucun validateur : c'est l'échappatoire assumée pour un chemin sur
/// volume amovible, et le gate autoritaire reste le `is_file()` d'[`ensure_hash_derived_image`].
pub(crate) const DOCKERFILE_PATH_ENV: &str = "PDO_SANDBOX_DOCKERFILE";

/// Tier env : `Some(path)` si un [`DOCKERFILE_PATH_ENV`] non vide est posé, sinon `None`.
pub(crate) fn env_dockerfile_path() -> Option<String> {
    std::env::var(DOCKERFILE_PATH_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Résolution 3-tiers **PURE** `profil → env → défaut de profil` — testable sans toucher
/// `std::env` (AC #431, étendue par #467 au tier `profile`, amputée du tier `stored` par #471).
/// Une valeur vide est traitée comme unset à TOUS les tiers.
///
/// Le tier `default` ne nomme pas de chemin en dur : il lit
/// [`crate::sandbox_profile::DEFAULT_PROFILE_IMAGE`], dont le `dockerfile: None` veut dire
/// « l'emplacement seedé », le seul défaut exprimable — un chemin littéral dans la constante
/// mentirait, puisqu'il dépend de `<sandbox_root>`, donc de `$HOME`.
pub(crate) fn resolve_dockerfile(
    profile: Option<&str>,
    env: Option<&str>,
    sandbox_root: &Path,
) -> ResolvedDockerfile {
    let default = match crate::sandbox_profile::DEFAULT_PROFILE_IMAGE.dockerfile {
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
    use std::os::unix::fs::PermissionsExt;

    /// Simple-quote une chaîne pour l'embarquer dans le script bash fake (D2 : aucune mutation
    /// d'env — le fake est threadé comme `docker_bin`).
    fn shell_single_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Comportement canné du faux `docker`, un exit code (+ stderr) par sous-commande. `Default` =
    /// **registry heureux** : image absente (inspect 1) → `pull` réussit (0) → `tag` réussit (0),
    /// build jamais atteint. Chaque test ne surcharge que les champs qui l'intéressent (#411).
    #[derive(Clone)]
    struct FakeSpec {
        inspect_exit: i32,
        build_exit: i32,
        build_stderr: String,
        pull_exit: i32,
        pull_stderr: String,
        tag_exit: i32,
    }

    impl Default for FakeSpec {
        fn default() -> Self {
            FakeSpec {
                inspect_exit: 1,
                build_exit: 0,
                build_stderr: String::new(),
                pull_exit: 0,
                pull_stderr: String::new(),
                tag_exit: 0,
            }
        }
    }

    /// Écrit un faux `docker` exécutable dans `dir` et renvoie `(bin, argv_log)`. `bin` est passé
    /// comme `docker_bin` ; `argv_log` accumule l'argv de chaque invocation (une ligne par arg),
    /// pour les assertions d'argv. Aucune mutation d'`std::env` (D2 : race parallèle cargo).
    ///
    /// Branche sur `$1` (`image`→inspect, `pull`→pull, `tag`→tag, `build`→build) et NON sur
    /// `"$1 $2"` : un vrai `docker build -t …` a `$2 = "-t"`, donc `"$1 $2" = "build -t"` ne
    /// matcherait pas `build`. Le stderr de `pull` (progression) et de `build` est configurable.
    fn write_fake_docker(dir: &Path, spec: &FakeSpec) -> (PathBuf, PathBuf) {
        let bin = dir.join("fake-docker");
        let argv_log = dir.join("argv.log");
        let script = format!(
            "#!/usr/bin/env bash\n\
             printf '%s\\n' \"$@\" >> \"{log}\"\n\
             case \"$1\" in\n\
             image) exit {inspect_exit} ;;\n\
             pull) printf '%s' {pull_stderr} >&2; exit {pull_exit} ;;\n\
             tag) exit {tag_exit} ;;\n\
             build) printf '%s' {build_stderr} >&2; exit {build_exit} ;;\n\
             *) exit 0 ;;\n\
             esac\n",
            log = argv_log.display(),
            inspect_exit = spec.inspect_exit,
            pull_stderr = shell_single_quote(&spec.pull_stderr),
            pull_exit = spec.pull_exit,
            tag_exit = spec.tag_exit,
            build_stderr = shell_single_quote(&spec.build_stderr),
            build_exit = spec.build_exit,
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin, argv_log)
    }

    /// Argv de la (dernière) invocation `build` extraite du log. Vide si aucun build : le build
    /// est la dernière invocation, on prend de la ligne `"build"` à la fin.
    fn build_argv(argv_log: &Path) -> Vec<String> {
        let content = std::fs::read_to_string(argv_log).unwrap_or_default();
        let lines: Vec<String> = content.lines().map(str::to_string).collect();
        match lines.iter().position(|l| l == "build") {
            Some(i) => lines[i..].to_vec(),
            None => Vec::new(),
        }
    }

    /// Regroupe le log d'argv plat en invocations : chaque appel docker que ce fake gère commence
    /// par un mot-clé de sous-commande connu (`image`/`pull`/`tag`/`build`) — une ligne qui en
    /// matche un ouvre une nouvelle invocation (les refs `pdo-sandbox:h-…`/`ghcr.io/…` ne sont
    /// jamais des mots-clés nus, donc pas de faux départ).
    fn invocations(argv_log: &Path) -> Vec<Vec<String>> {
        let content = std::fs::read_to_string(argv_log).unwrap_or_default();
        let starts = ["image", "pull", "tag", "build"];
        let mut invs: Vec<Vec<String>> = Vec::new();
        for line in content.lines() {
            if starts.contains(&line) {
                invs.push(vec![line.to_string()]);
            } else if let Some(last) = invs.last_mut() {
                last.push(line.to_string());
            }
        }
        invs
    }

    /// Argv de la première invocation dont `$1 == name` (`None` si absente).
    fn invocation(argv_log: &Path, name: &str) -> Option<Vec<String>> {
        invocations(argv_log)
            .into_iter()
            .find(|inv| inv.first().map(String::as_str) == Some(name))
    }

    fn docker_str(bin: &Path) -> String {
        bin.to_str().unwrap().to_string()
    }

    /// The pre-#431 `ensure_image` input: the seeded default location, `default` tier.
    /// Every legacy test threads this so its behaviour is pinned unchanged.
    fn seeded(sandbox_root: &Path) -> ResolvedDockerfile {
        resolve_dockerfile(None, None, sandbox_root)
    }

    /// A custom Dockerfile at `path`, as the `env` tier resolves it. Was the `stored` tier
    /// until #471 removed it; the env tier is now the surviving instance-wide one, and every
    /// custom-path assertion below is about the PATH, not about which tier named it.
    fn env_at(path: &Path, sandbox_root: &Path) -> ResolvedDockerfile {
        resolve_dockerfile(None, Some(path.to_str().unwrap()), sandbox_root)
    }

    /// Under `cargo test --workspace`, exec-ing a **freshly written** binary can
    /// return `ETXTBSY` (os error 26): a sibling test that `fork`+`exec`s at the
    /// same instant transiently inherits the write fd of this test's fake docker
    /// (rust-lang/rust#45719). Prod never exec-s a freshly-written binary
    /// (`docker` is stable), so the retry lives HERE, not in the core. Mirrors the
    /// identical guard in [`crate::sandbox_container`]'s tests.
    fn retry_etxtbsy<T>(mut op: impl FnMut() -> Result<T>) -> Result<T> {
        for _ in 0..100 {
            match op() {
                Err(e) if is_etxtbsy(&e) => {
                    std::thread::sleep(std::time::Duration::from_millis(5));
                }
                other => return other,
            }
        }
        op()
    }

    fn is_etxtbsy(e: &anyhow::Error) -> bool {
        e.chain().any(|c| {
            c.downcast_ref::<std::io::Error>()
                .and_then(std::io::Error::raw_os_error)
                == Some(26)
        })
    }

    #[test]
    fn present_image_skips_build() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 0,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(
            build_argv(&argv_log).is_empty(),
            "aucun build ne doit être lancé quand l'image est présente"
        );
    }

    #[test]
    fn absent_image_builds_then_returns_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert_eq!(
            build_argv(&argv_log),
            vec![
                "build".to_string(),
                "-t".to_string(),
                tag.clone(),
                "-f".to_string(),
                default_dockerfile_path(&sandbox_root).display().to_string(),
                build_context_dir(&sandbox_root).display().to_string(),
            ]
        );
    }

    #[test]
    fn build_failure_is_explicit_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                build_exit: 1,
                build_stderr: "boom: base image missing".to_string(),
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let err = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("failed to build the sandbox image"),
            "phrase de contexte manquante: {msg}"
        );
        assert!(
            msg.contains("boom: base image missing"),
            "stderr docker manquant (US-16 actionnable): {msg}"
        );
    }

    #[test]
    fn docker_binary_missing_errors_without_building() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox_root = tmp.path().join("sandbox");
        let missing = tmp.path().join("no-such-docker");

        let err = ensure_hash_derived_image(
            &docker_str(&missing),
            &sandbox_root,
            &seeded(&sandbox_root),
            ImageSource::Dockerfile,
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("Docker") && msg.contains("not found on PATH"),
            "message docker-absent attendu: {msg}"
        );
        // Chaîne à 2 maillons : la source `io::NotFound` est préservée (#298).
        assert!(
            err.chain().count() >= 2,
            "la source io::Error doit être préservée dans la chaîne anyhow"
        );
        // Le build ne doit jamais être atteint (la sonde échoue avant).
        assert!(
            !build_context_dir(&sandbox_root).exists(),
            "aucun contexte de build ne doit être créé quand docker est absent"
        );
    }

    #[test]
    fn seeds_dockerfile_when_absent_then_builds() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");
        assert!(!default_dockerfile_path(&sandbox_root).exists());

        retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        let seeded = std::fs::read(default_dockerfile_path(&sandbox_root)).unwrap();
        assert_eq!(
            seeded,
            EMBEDDED_DOCKERFILE.as_bytes(),
            "le Dockerfile seedé doit être identique à l'embarqué"
        );
    }

    #[test]
    fn edited_on_disk_dockerfile_is_preserved_and_drives_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");
        // Pré-écrire un Dockerfile ÉDITÉ (différent de l'embarqué).
        std::fs::create_dir_all(&sandbox_root).unwrap();
        let edited: &[u8] = b"FROM ubuntu:24.04\nRUN echo edited\n";
        std::fs::write(default_dockerfile_path(&sandbox_root), edited).unwrap();

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        // (a) Octets inchangés : pas d'écrasement.
        assert_eq!(
            std::fs::read(default_dockerfile_path(&sandbox_root)).unwrap(),
            edited,
            "le seed ne doit jamais écraser un Dockerfile existant"
        );
        // (b) Tag + argv reflètent le hash des octets ÉDITÉS, pas de l'embarqué.
        assert_eq!(tag, local_image_ref(BASE_IMAGE_NAME, edited));
        assert_ne!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(build_argv(&argv_log).contains(&tag));
    }

    #[test]
    fn build_context_is_not_sandbox_root() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        let argv = build_argv(&argv_log);
        let ctx = argv.last().unwrap();
        assert_eq!(ctx, &build_context_dir(&sandbox_root).display().to_string());
        assert_ne!(ctx, &sandbox_root.display().to_string(), "piège D8");
        assert!(ctx.ends_with(".build-ctx"));
    }

    #[test]
    fn dockerfile_tag_stable_and_edit_sensitive() {
        let base: &[u8] = b"FROM ubuntu:24.04\nRUN apt-get update\n";
        // Stable pour un contenu identique.
        assert_eq!(
            local_image_ref(BASE_IMAGE_NAME, base),
            local_image_ref(BASE_IMAGE_NAME, base)
        );
        // Change à l'édition.
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

    // -- #411 : chemin hybride registry (pull → retag / fallback build) ---------

    #[test]
    fn registry_pull_ok_retags_and_skips_build() {
        let tmp = tempfile::tempdir().unwrap();
        // Default = registry heureux : inspect 1 (absent) → pull 0 → tag 0.
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        let local_ref = local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes());
        let registry_ref = registry_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes());
        assert_eq!(tag, local_ref);
        // `pull` fut invoqué sur le ref registry content-addressé.
        assert_eq!(
            invocation(&argv_log, "pull"),
            Some(vec!["pull".to_string(), registry_ref.clone()]),
            "pull must target the registry ref"
        );
        // `tag` retague registry_ref → local_ref (l'invariant sandbox_container).
        assert_eq!(
            invocation(&argv_log, "tag"),
            Some(vec![
                "tag".to_string(),
                registry_ref.clone(),
                local_ref.clone()
            ]),
            "tag must retag the pulled ref under the local ref"
        );
        // Aucun build : le pull a réussi.
        assert!(
            build_argv(&argv_log).is_empty(),
            "a successful pull must skip the build; log:\n{}",
            std::fs::read_to_string(&argv_log).unwrap_or_default()
        );
    }

    #[test]
    fn registry_pull_ok_returns_local_not_registry_ref() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        // Retour TOUJOURS le ref local (prouve sandbox_container 0-change) — jamais le ref GHCR.
        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(
            tag.starts_with("pdo-sandbox:h-"),
            "must return the local ref: {tag}"
        );
        assert!(
            !tag.contains("ghcr.io"),
            "must never leak the registry ref to sandbox_container: {tag}"
        );
    }

    #[test]
    fn registry_pull_ok_ignores_stderr_progress() {
        let tmp = tempfile::tempdir().unwrap();
        // `docker pull` writes progress to stderr while succeeding (exit 0). Progress
        // is NOT a failure signal — only the exit code counts.
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                pull_stderr: "h-abc: Pulling from loulen/pdo-sandbox\nStatus: Downloaded\n"
                    .to_string(),
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(invocation(&argv_log, "tag").is_some(), "must still retag");
        assert!(
            build_argv(&argv_log).is_empty(),
            "stderr progress must not trigger a build"
        );
    }

    #[test]
    fn registry_pull_fail_falls_back_to_build() {
        let tmp = tempfile::tempdir().unwrap();
        // Pull fails (offline / 404 / registry down) → fallback build.
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                pull_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(
            invocation(&argv_log, "pull").is_some(),
            "pull was attempted"
        );
        assert!(
            invocation(&argv_log, "tag").is_none(),
            "a failed pull must NOT retag"
        );
        assert!(
            !build_argv(&argv_log).is_empty(),
            "a failed pull must fall back to a local build; log:\n{}",
            std::fs::read_to_string(&argv_log).unwrap_or_default()
        );
    }

    #[test]
    fn registry_pull_fail_then_build_fail_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                pull_exit: 1,
                build_exit: 1,
                build_stderr: "boom: base image missing".to_string(),
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let err = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("failed to build the sandbox image"),
            "pull-then-build-fail must surface the build error (US-16): {msg}"
        );
        assert!(
            msg.contains("boom: base image missing"),
            "the docker build stderr must be preserved: {msg}"
        );
    }

    #[test]
    fn dockerfile_mode_never_pulls() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(
            !build_argv(&argv_log).is_empty(),
            "dockerfile mode builds locally"
        );
        assert!(
            invocation(&argv_log, "pull").is_none(),
            "dockerfile mode must NEVER pull"
        );
        assert!(
            invocation(&argv_log, "tag").is_none(),
            "dockerfile mode must NEVER retag"
        );
    }

    #[test]
    fn local_present_skips_network_in_registry_mode() {
        let tmp = tempfile::tempdir().unwrap();
        // inspect 0 → image already local → fast-path returns before any network.
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 0,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(
            invocation(&argv_log, "pull").is_none(),
            "fast-path must skip pull (offline-safe reuse)"
        );
        assert!(
            invocation(&argv_log, "tag").is_none(),
            "fast-path must skip tag"
        );
        assert!(
            build_argv(&argv_log).is_empty(),
            "fast-path must skip build"
        );
    }

    #[test]
    fn docker_missing_errors_in_registry_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox_root = tmp.path().join("sandbox");
        let missing = tmp.path().join("no-such-docker");

        // Even in registry mode, Docker-absent is a hard error at the fast-path probe
        // — never a silent host fallback (US-16). Same guarantee as dockerfile mode.
        let err = ensure_hash_derived_image(
            &docker_str(&missing),
            &sandbox_root,
            &seeded(&sandbox_root),
            ImageSource::Registry,
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("Docker") && msg.contains("not found on PATH"),
            "docker-absent message expected: {msg}"
        );
        assert!(
            err.chain().count() >= 2,
            "the io::Error source must be preserved in the anyhow chain"
        );
        assert!(
            !build_context_dir(&sandbox_root).exists(),
            "no build context must be created when docker is absent"
        );
    }

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
        // as_str round-trips both variants.
        assert_eq!(ImageSource::Registry.as_str(), "registry");
        assert_eq!(ImageSource::Dockerfile.as_str(), "dockerfile");

        // #471: the resolver is PURE — `env → défaut de profil`, two tiers, no `std::env`.
        assert_eq!(
            resolve_image_source(None),
            crate::sandbox_profile::DEFAULT_PROFILE_IMAGE.source,
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
        let d = crate::sandbox_profile::DEFAULT_PROFILE_IMAGE;
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
    fn custom_dockerfile_drives_the_tag_and_the_build_f_flag() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");
        let custom = tmp
            .path()
            .join("repo")
            .join("docker")
            .join("sbx.Dockerfile");
        std::fs::create_dir_all(custom.parent().unwrap()).unwrap();
        let bytes: &[u8] = b"FROM ubuntu:24.04\nRUN echo custom-431\n";
        std::fs::write(&custom, bytes).unwrap();

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &env_at(&custom, &sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        // The tag is the hash of the CUSTOM bytes, never the seeded ones.
        assert_eq!(tag, local_image_ref(BASE_IMAGE_NAME, bytes));
        assert_ne!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        // The build points `-f` at the custom path, with the SAME empty context (D2:
        // a custom Dockerfile must be self-contained — no COPY).
        assert_eq!(
            build_argv(&argv_log),
            vec![
                "build".to_string(),
                "-t".to_string(),
                tag.clone(),
                "-f".to_string(),
                custom.display().to_string(),
                build_context_dir(&sandbox_root).display().to_string(),
            ]
        );
    }

    #[test]
    fn custom_dockerfile_skips_the_pull_in_registry_mode() {
        let tmp = tempfile::tempdir().unwrap();
        // Registry-happy fake: were a pull attempted it would SUCCEED and retag, so the
        // absence of `pull`/`tag` below is a real signal, not a coincidence.
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");
        let custom = tmp.path().join("custom.Dockerfile");
        std::fs::write(&custom, b"FROM ubuntu:24.04\nRUN echo custom\n").unwrap();

        retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &env_at(&custom, &sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert!(
            invocation(&argv_log, "pull").is_none(),
            "a custom Dockerfile's hash cannot exist upstream — no pull; log:\n{}",
            std::fs::read_to_string(&argv_log).unwrap_or_default()
        );
        assert!(invocation(&argv_log, "tag").is_none(), "no pull ⇒ no retag");
        assert!(
            !build_argv(&argv_log).is_empty(),
            "it builds locally instead"
        );
    }

    #[test]
    fn edited_seeded_dockerfile_still_attempts_a_pull_in_registry_mode() {
        // PINS THE NON-CHANGE (ADR-0030 §5 as amended by #431): the skip-pull predicate
        // reads the PATH, not the bytes. A Dockerfile edited in place at the default
        // location keeps trying the pull (which 404s and falls back to the build) —
        // exactly the pre-#431 behaviour. A bytes-based predicate would break every
        // machine that has ever updated PDO, whose on-disk Dockerfile comes from an
        // earlier release and whose tag DOES exist on GHCR.
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                pull_exit: 1, // as a 404 for an unpublished hash would behave
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");
        std::fs::create_dir_all(&sandbox_root).unwrap();
        let edited: &[u8] = b"FROM ubuntu:24.04\nRUN echo edited-in-place\n";
        std::fs::write(default_dockerfile_path(&sandbox_root), edited).unwrap();

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(BASE_IMAGE_NAME, edited));
        assert_eq!(
            invocation(&argv_log, "pull"),
            Some(vec![
                "pull".to_string(),
                registry_image_ref(BASE_IMAGE_NAME, edited)
            ]),
            "an edited SEEDED Dockerfile must still attempt the pull (path predicate)"
        );
        assert!(
            !build_argv(&argv_log).is_empty(),
            "the 404 falls back to the local build"
        );
    }

    #[test]
    fn missing_custom_dockerfile_is_a_hard_error_naming_path_and_tier() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");
        let missing = tmp.path().join("nope").join("Dockerfile");

        let err = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &env_at(&missing, &sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains(&missing.display().to_string()),
            "the reason must name the path (US-16 actionable): {msg}"
        );
        assert!(
            msg.contains("`env` tier"),
            "the reason must name the WINNING TIER: {msg}"
        );
        assert!(
            msg.contains(DOCKERFILE_PATH_ENV),
            "…and the remediation must name the knob that CAN be fixed, which since #471 is \
             the env var and no longer a setting: {msg}"
        );
        // No silent fallback to the seeded default: nothing was built, nothing probed.
        assert!(
            build_argv(&argv_log).is_empty(),
            "a missing custom path must never fall back to building the seeded default"
        );
        assert!(
            invocation(&argv_log, "image").is_none(),
            "the is_file() bail must precede the image_exists fast-path (the tag IS the \
             hash of those bytes, so without them the image is unnameable)"
        );
        assert!(
            !build_context_dir(&sandbox_root).exists(),
            "no build context must be created"
        );
    }

    #[test]
    fn a_dockerfile_path_pointing_at_a_directory_is_an_error() {
        // The `exists()` vs `is_file()` trap: `Path::exists()` is TRUE for a directory.
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");
        let dir = tmp.path().join("a-directory");
        std::fs::create_dir(&dir).unwrap();

        let err = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &env_at(&dir, &sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("regular file") && msg.contains(&dir.display().to_string()),
            "a directory must be rejected as not-a-regular-file: {msg}"
        );
    }

    #[test]
    fn seed_lands_at_the_default_path_even_while_a_custom_one_is_in_use() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 1,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");
        let custom = tmp.path().join("custom.Dockerfile");
        let custom_bytes: &[u8] = b"FROM ubuntu:24.04\nRUN echo custom\n";
        std::fs::write(&custom, custom_bytes).unwrap();
        assert!(!default_dockerfile_path(&sandbox_root).exists());

        retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &env_at(&custom, &sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        // The seed materialises the `default` tier (so clearing the setting lands on a
        // real file) at the DEFAULT location…
        assert_eq!(
            std::fs::read(default_dockerfile_path(&sandbox_root)).unwrap(),
            EMBEDDED_DOCKERFILE.as_bytes(),
            "the seed must still land at the default path"
        );
        // …and NEVER writes to the custom path (that would mutate a repo the user only
        // POINTED at).
        assert_eq!(
            std::fs::read(&custom).unwrap(),
            custom_bytes,
            "the seed must never overwrite the custom Dockerfile"
        );
    }

    // -- #466 : le NOM d'image est une donnée de la variante ------------------

    /// Le Dockerfile de variante LIVRÉ, lu au build du test uniquement (`#[cfg(test)]`) : il n'a
    /// pas à grossir le binaire, personne ne le seede — c'est un profil (ou l'env) qui le pointe.
    const EMBEDDED_CHROME_DEV_DOCKERFILE: &str =
        include_str!("../assets/sandbox/Dockerfile.chrome-dev");

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
                "/repo/crates/pdo-daemon/assets/sandbox/Dockerfile.chrome-dev"
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

    #[test]
    fn variant_dockerfile_drives_the_image_name_and_the_tag() {
        let tmp = tempfile::tempdir().unwrap();
        // Fake registry-heureux : un pull, s'il était tenté, RÉUSSIRAIT — l'absence de `pull`
        // ci-dessous est donc un signal, pas une coïncidence.
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");
        let variant = tmp.path().join("assets").join("Dockerfile.chrome-dev");
        std::fs::create_dir_all(variant.parent().unwrap()).unwrap();
        let bytes: &[u8] = b"FROM ubuntu:24.04\nRUN echo chrome-dev\n";
        std::fs::write(&variant, bytes).unwrap();

        let tag = retry_etxtbsy(|| {
            ensure_hash_derived_image(
                &docker_str(&docker),
                &sandbox_root,
                &env_at(&variant, &sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        // AC 2 : le tag porte le NOM de la variante, et le hash de SES octets.
        assert_eq!(
            tag,
            "pdo-sandbox-chrome-dev:h-".to_string() + &dockerfile_hash(bytes)
        );
        assert_eq!(tag, local_image_ref("pdo-sandbox-chrome-dev", bytes));
        assert_ne!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, bytes),
            "le nom ne doit pas retomber sur celui de la base"
        );
        // C'est bien ce tag que `docker build -t` reçoit (donc ce que `docker inspect` montrera).
        assert_eq!(
            build_argv(&argv_log),
            vec![
                "build".to_string(),
                "-t".to_string(),
                tag.clone(),
                "-f".to_string(),
                variant.display().to_string(),
                build_context_dir(&sandbox_root).display().to_string(),
            ]
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

    // -- #467 : la source d'image appartient au profil de staging -------------

    fn profile_dockerfile(path: &Path) -> ProfileImage {
        ProfileImage::Dockerfile {
            path: path.to_str().unwrap().to_string(),
        }
    }

    fn explicit(image_ref: &str) -> ImagePlan {
        ImagePlan::ExplicitRef {
            image_ref: image_ref.to_string(),
            profile: "chrome".to_string(),
        }
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

    /// AC3, la moitié positive du contrat : un ref explicite est tiré tel quel, rendu tel quel, et
    /// **jamais** retagué — il n'y a pas de second nom sous lequel le poser.
    #[test]
    fn an_explicit_ref_is_pulled_and_returned_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        // Absent localement (inspect 1) puis pull OK (0).
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &explicit("ghcr.io/acme/agent:1.4"),
            )
        })
        .unwrap();

        assert_eq!(tag, "ghcr.io/acme/agent:1.4");
        assert_eq!(
            invocation(&argv_log, "pull"),
            Some(vec![
                "pull".to_string(),
                "ghcr.io/acme/agent:1.4".to_string()
            ])
        );
        assert!(
            invocation(&argv_log, "tag").is_none(),
            "un ref explicite n'a pas de second nom : jamais de retag en h-<hash>"
        );
        assert!(
            build_argv(&argv_log).is_empty(),
            "aucun build : il n'y a pas de Dockerfile dans cette histoire"
        );
        // Zéro IO : ni Dockerfile seedé, ni contexte de build.
        assert!(
            !default_dockerfile_path(&sandbox_root).exists(),
            "cette branche ne touche pas le disque"
        );
        assert!(!build_context_dir(&sandbox_root).exists());
    }

    /// AC3 : un ref inexistant est une erreur DURE qui NOMME le ref, et **aucun `docker build`**
    /// n'est lancé. Le fake est réglé pour que le build RÉUSSIRAIT s'il était tenté — son absence
    /// est donc un signal, pas une coïncidence.
    #[test]
    fn a_missing_explicit_ref_is_a_hard_error_and_never_builds() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                pull_exit: 1,
                pull_stderr: "Error response from daemon: manifest unknown".to_string(),
                build_exit: 0,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let err = retry_etxtbsy(|| {
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &explicit("ghcr.io/acme/nope:9"),
            )
        })
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("ghcr.io/acme/nope:9"),
            "la reason DOIT nommer le ref (AC3, US-16): {msg}"
        );
        assert!(
            msg.contains("chrome"),
            "…et le profil qui l'a demandé, sinon rien ne le dit: {msg}"
        );
        assert!(
            msg.contains("manifest unknown"),
            "le stderr de docker est la seule chose actionnable qui reste: {msg}"
        );
        assert!(
            build_argv(&argv_log).is_empty(),
            "AUCUN build ne doit être lancé — pas de hash, donc pas de repli; log:\n{}",
            std::fs::read_to_string(&argv_log).unwrap_or_default()
        );
        assert!(
            !build_context_dir(&sandbox_root).exists(),
            "et donc aucun contexte de build créé"
        );
    }

    /// Fast-path offline : un ref déjà local (tiré hier, ou buildé à la main sous ce nom) ne
    /// déclenche aucun réseau. Le fake tirerait avec succès — l'absence de `pull` est un signal.
    #[test]
    fn a_locally_present_explicit_ref_skips_the_network() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(
            tmp.path(),
            &FakeSpec {
                inspect_exit: 0,
                ..Default::default()
            },
        );
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &explicit("local/img:1"),
            )
        })
        .unwrap();

        assert_eq!(tag, "local/img:1");
        assert_eq!(
            invocation(&argv_log, "image"),
            Some(vec![
                "image".to_string(),
                "inspect".to_string(),
                "local/img:1".to_string()
            ])
        );
        assert!(
            invocation(&argv_log, "pull").is_none(),
            "le fast-path précède TOUT réseau"
        );
        assert!(build_argv(&argv_log).is_empty());
    }

    /// Docker absent reste une erreur dure sur cette branche aussi : jamais de repli hôte
    /// silencieux (US-16), y compris quand il n'y a rien à builder.
    #[test]
    fn docker_missing_errors_on_an_explicit_ref_too() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox_root = tmp.path().join("sandbox");
        let missing = tmp.path().join("no-such-docker");

        let err = ensure_image(
            &docker_str(&missing),
            &sandbox_root,
            &explicit("ghcr.io/acme/agent:1.4"),
        )
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("Docker") && msg.contains("not found on PATH"),
            "message docker-absent attendu: {msg}"
        );
        assert!(err.chain().count() >= 2);
    }

    /// Le plan hash-dérivé passé par le MÊME point d'entrée donne exactement ce que
    /// `ensure_hash_derived_image` donne en direct : l'aiguillage n'ajoute rien (c'est ce qui rend
    /// les pins historiques de ce fichier encore valables pour la prod).
    #[test]
    fn the_dispatcher_is_transparent_for_a_hash_derived_plan() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");

        let tag = retry_etxtbsy(|| {
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &ImagePlan::HashDerived {
                    dockerfile: seeded(&sandbox_root),
                    source: ImageSource::Registry,
                },
            )
        })
        .unwrap();

        assert_eq!(
            tag,
            local_image_ref(BASE_IMAGE_NAME, EMBEDDED_DOCKERFILE.as_bytes())
        );
        assert!(
            invocation(&argv_log, "tag").is_some(),
            "pull → retag, comme avant"
        );
    }

    /// Un chemin de profil manquant échoue en nommant le tier `profile` — et la remédiation
    /// renvoie vers le PROFIL, pas vers l'env var, qui n'est pas le tier gagnant.
    #[test]
    fn a_missing_profile_dockerfile_names_the_profile_tier() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");
        let missing = tmp.path().join("nope").join("Dockerfile.variant");

        let err = retry_etxtbsy(|| {
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &ImagePlan::HashDerived {
                    dockerfile: resolve_dockerfile(
                        Some(missing.to_str().unwrap()),
                        None,
                        &sandbox_root,
                    ),
                    source: ImageSource::Registry,
                },
            )
        })
        .unwrap_err();

        let msg = format!("{err:#}");
        assert!(msg.contains("`profile` tier"), "{msg}");
        assert!(msg.contains(&missing.display().to_string()), "{msg}");
        assert!(
            msg.contains("staging profile"),
            "la remédiation doit pointer le profil, pas l'env var: {msg}"
        );
        assert!(
            !msg.contains(DOCKERFILE_PATH_ENV),
            "…et surtout PAS l'env var, que l'utilisateur n'a pas posée: {msg}"
        );
        assert!(build_argv(&argv_log).is_empty());
    }

    // -- Slice D (#410): Docker availability probe ---------------------------

    /// Write a fake `docker` whose `version` subcommand exits with `version_exit`.
    /// Threaded as `docker_bin` (no `std::env` mutation, D2 discipline).
    fn write_fake_docker_version(dir: &Path, version_exit: i32) -> PathBuf {
        let bin = dir.join("fake-docker-version");
        let script = format!(
            "#!/usr/bin/env bash\n\
             case \"$1\" in\n\
             version) echo '27.0.0'; exit {version_exit} ;;\n\
             *) exit 0 ;;\n\
             esac\n",
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    /// Probe a freshly-written fake `docker`, retrying past a transient `NotFound`.
    /// Exec-ing a just-written binary under `cargo test --workspace` can hit
    /// `ETXTBSY` (rust-lang#45719), which `probe_docker` collapses to the
    /// "not installed" verdict — indistinguishable, for an existing binary, from a
    /// transient race. Since the binary IS on disk, a `NotFound` here means the
    /// race; retry until the real verdict settles. (The genuinely-absent test uses a
    /// path that never exists, so it does NOT go through this helper.)
    fn probe_stable(bin: &Path) -> DockerProbe {
        for _ in 0..100 {
            let p = probe_docker(&docker_str(bin));
            if p.reason.as_deref() != Some(DOCKER_NOT_FOUND_MSG) {
                return p;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        probe_docker(&docker_str(bin))
    }

    #[test]
    fn probe_docker_reports_available_on_exit_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let docker = write_fake_docker_version(tmp.path(), 0);
        let probe = probe_stable(&docker);
        assert!(probe.available, "exit 0 must report available");
        assert!(probe.reason.is_none());
    }

    #[test]
    fn probe_docker_reports_daemon_unreachable_on_nonzero_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let docker = write_fake_docker_version(tmp.path(), 1);
        let probe = probe_stable(&docker);
        assert!(!probe.available, "a non-zero exit must report unavailable");
        assert_eq!(probe.reason.as_deref(), Some(DOCKER_DAEMON_UNREACHABLE_MSG));
    }

    #[test]
    fn probe_docker_reports_not_found_when_binary_absent() {
        // A path that does not exist → spawn `NotFound` → the "not installed" message.
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-docker");
        let probe = probe_docker(missing.to_str().unwrap());
        assert!(!probe.available);
        assert_eq!(probe.reason.as_deref(), Some(DOCKER_NOT_FOUND_MSG));
    }
}
