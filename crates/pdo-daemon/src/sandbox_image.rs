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

#![allow(dead_code)] // Tracer bullet : consommé par #406/#407, non câblé dans cette slice.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};
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

/// Ref locale `pdo-sandbox:h-<hash>`. (GHCR #411 formate son propre préfixe autour du même hash.)
pub(crate) fn local_image_ref(dockerfile_bytes: &[u8]) -> String {
    format!("pdo-sandbox:h-{}", dockerfile_hash(dockerfile_bytes))
}

/// Namespace GHCR de l'image publiée (#411). Owner lowercasé (GHCR rejette l'uppercase).
/// MÊME hash que [`local_image_ref`] → pull et build local interchangeables sous le même contenu
/// (ADR-0030 pt 7). `release.yml` construit ce même chemin en bash (`${GITHUB_REPOSITORY_OWNER,,}`).
pub(crate) const REGISTRY_NAMESPACE: &str = "ghcr.io/loulen/pdo-sandbox";

/// Ref registry `ghcr.io/loulen/pdo-sandbox:h-<hash>` (MÊME hash que [`local_image_ref`], donc pull
/// et build sont interchangeables sous le ref local après retag).
pub(crate) fn registry_image_ref(dockerfile_bytes: &[u8]) -> String {
    format!(
        "{REGISTRY_NAMESPACE}:h-{}",
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

/// `docker pull <registry_ref>` (réseau, image PUBLIQUE, sans auth) : `Ok(true)` si exit 0 (tirée),
/// `Ok(false)` si exit != 0 (offline / 404 tag absent / registry down → fallback build). `docker`
/// introuvable (spawn `NotFound`) → `Err` explicite (jamais de fallback silencieux masquant Docker
/// absent — miroir strict d'[`image_exists`]). Le stderr de PROGRESSION de `docker pull` n'est PAS
/// un signal d'échec : seul l'exit code compte.
pub(crate) fn pull_image(docker_bin: &str, registry_ref: &str) -> Result<bool> {
    match Command::new(docker_bin)
        .args(["pull", registry_ref])
        .output()
    {
        Ok(output) => Ok(output.status.success()),
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

/// Provisionneur idempotent **hybride** (seul point d'entrée de #406/#407) : garantit que le ref
/// local `pdo-sandbox:h-<hash>` existe et le retourne TOUJOURS (invariant `sandbox_container`).
///
/// Ordre (D7) : seed → **contrôle `is_file()` du chemin résolu** → lit octets → `local_ref` →
/// **fast-path `image_exists(local_ref)` (zéro réseau, offline-safe)** → si
/// [`ImageSource::Registry`] **et** emplacement par défaut : `docker pull` le `registry_ref`, OK →
/// `docker tag` vers `local_ref` → retour ; pull raté → fallthrough vers le build local → retour ;
/// build KO → `Err`. [`ImageSource::Dockerfile`] : build direct, **jamais** de pull.
///
/// **Sync délibéré (D3)** : `docker build`/`docker pull` sont bloquants et longs, leur place est
/// dans le `spawn_blocking` du caller async (#407), pas dans une tâche tokio ; garder ce module
/// sync laisse aussi les tests en `#[test]` simples.
pub(crate) fn ensure_image(
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
        anyhow::bail!(
            "the sandbox Dockerfile resolved from the `{}` tier does not exist or is not a \
             regular file: {} — fix `dockerfile_path` or clear it to fall back to the \
             seeded default at {}",
            dockerfile.source.as_str(),
            path.display(),
            default_dockerfile_path(sandbox_root).display(),
        );
    }
    // 3. Octets bruts sur disque = entrée EXACTE du hash ET du build (jamais normaliser).
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read sandbox Dockerfile at {}", path.display()))?;
    // 4. Ref local content-addressé = TOUJOURS la valeur de retour (invariant sandbox_container).
    let local_ref = local_image_ref(&bytes);
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
        let registry_ref = registry_image_ref(&bytes);
        if pull_image(docker_bin, &registry_ref)? {
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

/// D'où [`ensure_image`] tire l'image (#411). **Par-daemon**, PAS par-Run : contrairement à
/// [`crate::event_log::SandboxMode`], NE PAS la porter sur `RunStarted`. Défini dans ce module
/// feuille (provisionnement) ; le sens de dépendance config → sandbox_image existe déjà → 0 cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ImageSource {
    /// Pull `ghcr.io/loulen/pdo-sandbox:h-<hash>`, retag local, build en fallback. **Défaut.**
    #[default]
    Registry,
    /// Ne jamais tirer : build local depuis le Dockerfile seedé (comportement #405).
    Dockerfile,
}

impl ImageSource {
    /// Le tier défaut (jamais `None`), surfacé par `GET /settings`.
    pub(crate) const DEFAULT: ImageSource = ImageSource::Registry;

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ImageSource::Registry => "registry",
            ImageSource::Dockerfile => "dockerfile",
        }
    }

    /// Parse la forme filaire ; `None` pour tout token inconnu (le validateur PUT les rejette ;
    /// le résolveur les traite comme unset défensivement). Miroir de `ServiceHealthOverride::parse`.
    pub(crate) fn parse(s: &str) -> Option<ImageSource> {
        match s.trim().to_ascii_lowercase().as_str() {
            "registry" => Some(ImageSource::Registry),
            "dockerfile" => Some(ImageSource::Dockerfile),
            _ => None,
        }
    }
}

/// Env var overridant la source stockée (tier optionnel). Lue UNE fois au bord, jamais dans le
/// cœur — miroir de [`DOCKER_CMD_OVERRIDE_ENV`].
pub(crate) const IMAGE_SOURCE_ENV: &str = "PDO_SANDBOX_IMAGE_SOURCE";

/// Tier env pour la disclosure `GET /settings` : `Some("registry"|"dockerfile")` si un
/// `PDO_SANDBOX_IMAGE_SOURCE` valide est posé, sinon `None` (unset/invalide).
pub(crate) fn env_image_source() -> Option<String> {
    std::env::var(IMAGE_SOURCE_ENV)
        .ok()
        .as_deref()
        .and_then(ImageSource::parse)
        .map(|s| s.as_str().to_string())
}

/// Source effective, précédence `stored → env → default(Registry)` (#411, ADR-0015). Une valeur
/// stockée vide/invalide est traitée comme unset (miroir de la sentinelle `""` + du validateur PUT).
/// SOURCE UNIQUE consommée par [`ensure_image`] ET `build_settings_view` (0 drift, leçon #373).
pub(crate) fn image_source_with(stored: Option<String>) -> ImageSource {
    stored
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(ImageSource::parse)
        .or_else(|| env_image_source().and_then(|s| ImageSource::parse(&s)))
        .unwrap_or(ImageSource::DEFAULT)
}

// -- Dockerfile résolu : réglage 3-tiers (#431) -------------------------------

/// Quel tier a choisi le Dockerfile (#431). Miroir de la discipline `as_str` d'[`ImageSource`] ;
/// surfacé par `GET /settings` **ET** par la `reason` du `RunFailed` — qui regarde un
/// « no such file » a besoin de savoir QUI l'a dit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DockerfileSource {
    Stored,
    Env,
    Default,
}

impl DockerfileSource {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            DockerfileSource::Stored => "stored",
            DockerfileSource::Env => "env",
            DockerfileSource::Default => "default",
        }
    }
}

/// Le Dockerfile que [`ensure_image`] hashe et builde, résolu UNE fois au bord (#431).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolvedDockerfile {
    pub(crate) path: PathBuf,
    pub(crate) source: DockerfileSource,
    /// Prédicat de skip-pull (ADR-0030 §5, précisé par #431) : porte sur l'EMPLACEMENT par
    /// défaut, **pas** sur le tier — un utilisateur peut épingler le chemin par défaut via le
    /// picker, et ça doit continuer à puller. Égalité `PathBuf` volontairement nue :
    /// `canonicalize` est de l'IO, échoue sur un chemin absent, et empoisonnerait la pureté du
    /// résolveur. Mal classer est inoffensif dans les deux sens (un 404 gâché, ou un pull évité
    /// qui aurait 404 de toute façon) — le skip-pull est une optimisation, pas un gate de
    /// correction.
    pub(crate) is_default_location: bool,
}

/// Env var pointant le Dockerfile de la sandbox (tier optionnel, #431). Lue UNE fois au bord,
/// jamais dans le cœur — miroir de [`IMAGE_SOURCE_ENV`]. Contourne par construction la validation
/// de `PUT /settings` : c'est l'échappatoire assumée pour un chemin sur volume amovible ; les deux
/// tiers restent gatés au prep.
pub(crate) const DOCKERFILE_PATH_ENV: &str = "PDO_SANDBOX_DOCKERFILE";

/// Tier env pour la disclosure `GET /settings` : `Some(path)` si un [`DOCKERFILE_PATH_ENV`] non
/// vide est posé, sinon `None`.
pub(crate) fn env_dockerfile_path() -> Option<String> {
    std::env::var(DOCKERFILE_PATH_ENV)
        .ok()
        .filter(|s| !s.is_empty())
}

/// Résolution 3-tiers **PURE** — testable sans toucher `std::env` (AC #431 : « précédence testée
/// stored / env / défaut »). Une valeur vide est traitée comme unset aux deux tiers (miroir de la
/// sentinelle `""` et du validateur PUT).
///
/// Ce découpage pur/bord est une amélioration délibérée sur [`image_source_with`], qui lit l'env
/// dans lui-même et dont le test ne peut donc couvrir que stored+default.
pub(crate) fn resolve_dockerfile(
    stored: Option<&str>,
    env: Option<&str>,
    sandbox_root: &Path,
) -> ResolvedDockerfile {
    let default = default_dockerfile_path(sandbox_root);
    let (path, source) = match stored.filter(|s| !s.is_empty()) {
        Some(p) => (PathBuf::from(p), DockerfileSource::Stored),
        None => match env.filter(|s| !s.is_empty()) {
            Some(p) => (PathBuf::from(p), DockerfileSource::Env),
            None => (default.clone(), DockerfileSource::Default),
        },
    };
    ResolvedDockerfile {
        is_default_location: path == default,
        path,
        source,
    }
}

/// Wrapper de bord : lit [`DOCKERFILE_PATH_ENV`] UNE fois puis délègue au résolveur pur.
/// SOURCE UNIQUE consommée par `sandbox_run::context_from_state` **ET** `build_settings_view`
/// (0 drift, leçon #373).
pub(crate) fn dockerfile_with(stored: Option<String>, sandbox_root: &Path) -> ResolvedDockerfile {
    resolve_dockerfile(
        stored.as_deref(),
        env_dockerfile_path().as_deref(),
        sandbox_root,
    )
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

    /// A custom Dockerfile at `path`, as a `stored` tier would resolve it.
    fn stored_at(path: &Path, sandbox_root: &Path) -> ResolvedDockerfile {
        resolve_dockerfile(Some(path.to_str().unwrap()), None, sandbox_root)
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
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

        let err = ensure_image(
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
            ensure_image(
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
            ensure_image(
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
        assert_eq!(tag, local_image_ref(edited));
        assert_ne!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
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
        assert_eq!(local_image_ref(base), local_image_ref(base));
        // Change à l'édition.
        let edited: &[u8] = b"FROM ubuntu:24.04\nRUN apt-get update\nRUN apt-get install -y git\n";
        assert_ne!(dockerfile_hash(base), dockerfile_hash(edited));

        let h = dockerfile_hash(base);
        assert_eq!(h.len(), 12);
        assert!(h
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(local_image_ref(base).starts_with("pdo-sandbox:h-"));

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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        let local_ref = local_image_ref(EMBEDDED_DOCKERFILE.as_bytes());
        let registry_ref = registry_image_ref(EMBEDDED_DOCKERFILE.as_bytes());
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        // Retour TOUJOURS le ref local (prouve sandbox_container 0-change) — jamais le ref GHCR.
        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
        let err = ensure_image(
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
        // as_str round-trips both variants; the built-in default is Registry.
        assert_eq!(ImageSource::Registry.as_str(), "registry");
        assert_eq!(ImageSource::Dockerfile.as_str(), "dockerfile");
        assert_eq!(ImageSource::DEFAULT, ImageSource::Registry);

        // resolver: a concrete stored value wins; empty/invalid falls through to
        // env→default (no test sets PDO_SANDBOX_IMAGE_SOURCE, so default = Registry).
        assert_eq!(image_source_with(None), ImageSource::Registry);
        assert_eq!(
            image_source_with(Some(String::new())),
            ImageSource::Registry
        );
        assert_eq!(
            image_source_with(Some("ecr".to_string())),
            ImageSource::Registry
        );
        assert_eq!(
            image_source_with(Some("dockerfile".to_string())),
            ImageSource::Dockerfile
        );
        assert_eq!(
            image_source_with(Some("registry".to_string())),
            ImageSource::Registry
        );
    }

    // -- #431 : le Dockerfile résolu est un réglage 3-tiers -------------------

    #[test]
    fn resolve_dockerfile_precedence_stored_env_default() {
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);

        // default tier: nothing stored, nothing in env.
        let r = resolve_dockerfile(None, None, root);
        assert_eq!(r.path, default);
        assert_eq!(r.source, DockerfileSource::Default);
        assert!(r.is_default_location);

        // env tier wins over the default.
        let r = resolve_dockerfile(None, Some("/env/Dockerfile"), root);
        assert_eq!(r.path, Path::new("/env/Dockerfile"));
        assert_eq!(r.source, DockerfileSource::Env);
        assert!(!r.is_default_location);

        // stored tier wins over env (ADR-0015).
        let r = resolve_dockerfile(Some("/stored/Dockerfile"), Some("/env/Dockerfile"), root);
        assert_eq!(r.path, Path::new("/stored/Dockerfile"));
        assert_eq!(r.source, DockerfileSource::Stored);
        assert!(!r.is_default_location);
    }

    #[test]
    fn resolve_dockerfile_treats_empty_string_as_unset_at_both_tiers() {
        // Mirror of the `""` clear sentinel + the PUT validator: an empty value must never
        // win precedence at either tier.
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);

        let r = resolve_dockerfile(Some(""), None, root);
        assert_eq!(r.path, default);
        assert_eq!(r.source, DockerfileSource::Default);

        let r = resolve_dockerfile(Some(""), Some("/env/Dockerfile"), root);
        assert_eq!(
            r.source,
            DockerfileSource::Env,
            "empty stored falls through"
        );

        let r = resolve_dockerfile(None, Some(""), root);
        assert_eq!(r.path, default);
        assert_eq!(r.source, DockerfileSource::Default);
    }

    #[test]
    fn is_default_location_is_about_the_path_not_the_tier() {
        // THE tier-vs-path trap: pinning the DEFAULT path through the picker stores a
        // value, so the tier is `stored` — but the location is still the seeded one, so
        // the pull must still be attempted. `is_default_location` is path-math, not tier-math.
        let root = Path::new("/home/u/.pdo/sandbox");
        let default = default_dockerfile_path(root);
        let r = resolve_dockerfile(Some(default.to_str().unwrap()), None, root);
        assert_eq!(r.source, DockerfileSource::Stored);
        assert!(
            r.is_default_location,
            "a stored value pointing AT the default location must still pull"
        );
    }

    #[test]
    fn dockerfile_source_as_str_round_trips() {
        assert_eq!(DockerfileSource::Stored.as_str(), "stored");
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &stored_at(&custom, &sandbox_root),
                ImageSource::Dockerfile,
            )
        })
        .unwrap();

        // The tag is the hash of the CUSTOM bytes, never the seeded ones.
        assert_eq!(tag, local_image_ref(bytes));
        assert_ne!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &stored_at(&custom, &sandbox_root),
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &seeded(&sandbox_root),
                ImageSource::Registry,
            )
        })
        .unwrap();

        assert_eq!(tag, local_image_ref(edited));
        assert_eq!(
            invocation(&argv_log, "pull"),
            Some(vec!["pull".to_string(), registry_image_ref(edited)]),
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &stored_at(&missing, &sandbox_root),
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
            msg.contains("`stored` tier"),
            "the reason must name the WINNING TIER: {msg}"
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
    fn dockerfile_path_pointing_at_a_directory_is_an_error() {
        // The `exists()` vs `is_file()` trap: `Path::exists()` is TRUE for a directory.
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(tmp.path(), &FakeSpec::default());
        let sandbox_root = tmp.path().join("sandbox");
        let dir = tmp.path().join("a-directory");
        std::fs::create_dir(&dir).unwrap();

        let err = retry_etxtbsy(|| {
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &stored_at(&dir, &sandbox_root),
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
            ensure_image(
                &docker_str(&docker),
                &sandbox_root,
                &stored_at(&custom, &sandbox_root),
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
