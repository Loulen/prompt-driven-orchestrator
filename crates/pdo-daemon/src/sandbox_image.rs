//! Fourniture de l'image sandbox par build local (#405, slice B du PRD #403).
//!
//! Miroir de [`crate::worktree_ops`] / [`crate::sandbox_staging`] : pas d'`AppState`,
//! pas d'async, pas de lecture d'env dans le cœur — `&Path`/`&str`/`&[u8]` in,
//! path-math ou `std::fs`/`std::process::Command` out. `HOME` et le binaire docker
//! ne sont lus QUE par les résolveurs de bord.
//!
//! Ce module garantit qu'une image `pdo-sandbox:h-<hash>` (tag = hash du CONTENU du
//! Dockerfile) existe localement, en la buildant depuis le Dockerfile sur disque quand
//! elle est absente. Les slices sœurs le CONSOMMENT :
//! - #406 instancie un conteneur à partir de l'image ;
//! - #407 câble [`ensure_image`] dans le run-advance (ADR-0030) — via `spawn_blocking`
//!   car `docker build` est bloquant et long ;
//! - #411 ajoutera le pull GHCR en amont du build local, en réutilisant [`dockerfile_hash`].
//!
//! Le tag est **adressé par contenu**, pas versionné :
//! rationale (content-hash vs semver ; interchangeabilité pull #411 / build local)
//! -> ADR-0030 (#407).

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
const DOCKER_NOT_FOUND_MSG: &str =
    "sandbox run requires Docker, but the `docker` binary was not found on PATH — \
     install Docker or set this run's sandbox to `off`";

// -- path math (pur, sans IO) ------------------------------------------------

/// `<sandbox_root>/Dockerfile` — emplacement canonique du Dockerfile seedé/buildé.
pub(crate) fn dockerfile_path(sandbox_root: &Path) -> PathBuf {
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

/// Ref locale `pdo-sandbox:h-<hash>`. (GHCR #411 formatera son propre préfixe autour du même hash.)
pub(crate) fn local_image_ref(dockerfile_bytes: &[u8]) -> String {
    format!("pdo-sandbox:h-{}", dockerfile_hash(dockerfile_bytes))
}

// -- effets fs (sync std::fs, anyhow + .context) -----------------------------

/// Écrit le Dockerfile `embedded` à son chemin canonique **si absent** ; sinon **ne touche
/// à rien** (édition utilisateur préservée : une édition change le hash donc rebuild). Renvoie
/// le chemin dans les deux cas.
pub(crate) fn seed_dockerfile(sandbox_root: &Path, embedded: &str) -> Result<PathBuf> {
    let path = dockerfile_path(sandbox_root);
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
        Err(e) => Err(e)
            .context("failed to run `docker image inspect` while probing the sandbox image"),
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
            return Err(e)
                .context("failed to run `docker build` while building the sandbox image");
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

/// Provisionneur idempotent (seul point d'entrée de #406/#407) :
/// seed si absent -> lit octets disque -> tag -> présente ? `Ok(tag)` : build -> `Ok(tag)`.
///
/// **Sync délibéré (D3)** : `docker build` est un travail bloquant et long, sa place est dans le
/// `spawn_blocking` du caller async (#407), pas dans une tâche tokio ; garder ce module sync
/// laisse aussi les tests en `#[test]` simples.
pub(crate) fn ensure_image(docker_bin: &str, sandbox_root: &Path) -> Result<String> {
    // 1. Seed le Dockerfile si absent (édition utilisateur préservée).
    seed_dockerfile(sandbox_root, EMBEDDED_DOCKERFILE)?;
    // 2. Octets bruts sur disque = entrée EXACTE du hash ET du build (jamais normaliser).
    let dockerfile = dockerfile_path(sandbox_root);
    let bytes = std::fs::read(&dockerfile).with_context(|| {
        format!("failed to read sandbox Dockerfile at {}", dockerfile.display())
    })?;
    // 3. Tag adressé par contenu.
    let tag = local_image_ref(&bytes);
    // 4. Présente localement -> pas de build.
    if image_exists(docker_bin, &tag)? {
        return Ok(tag);
    }
    // 5. Contexte de build dédié VIDE (D8 : jamais sandbox_root, siblings = staging par-run).
    let context_dir = build_context_dir(sandbox_root);
    std::fs::create_dir_all(&context_dir).with_context(|| {
        format!("failed to create sandbox build context at {}", context_dir.display())
    })?;
    // 6. Build. v1: double-build concurrent premier-run accepté (deux `docker build -t <même
    //    tag>` sont sûrs — daemon sérialise + cache, la sonde court-circuite le 2e run) ;
    //    ajouter un lock par tag si ça mord.
    build_image(docker_bin, &tag, &dockerfile, &context_dir)?;
    // 7.
    Ok(tag)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Simple-quote une chaîne pour l'embarquer dans le script bash fake (D2 : aucune mutation
    /// d'env — le fake est threadé comme `docker_bin`).
    fn shell_single_quote(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Écrit un faux `docker` exécutable dans `dir` et renvoie `(bin, argv_log)`. `bin` est passé
    /// comme `docker_bin` ; `argv_log` accumule l'argv de chaque invocation (une ligne par arg),
    /// pour les assertions d'argv. Aucune mutation d'`std::env` (D2 : race parallèle cargo).
    ///
    /// Branche sur `$1` (`image` -> inspect, `build` -> build) et NON sur `"$1 $2"` : un vrai
    /// `docker build -t …` a `$2 = "-t"`, donc `"$1 $2" = "build -t"` ne matcherait pas `build`.
    fn write_fake_docker(
        dir: &Path,
        inspect_exit: i32,
        build_exit: i32,
        build_stderr: &str,
    ) -> (PathBuf, PathBuf) {
        let bin = dir.join("fake-docker");
        let argv_log = dir.join("argv.log");
        let script = format!(
            "#!/usr/bin/env bash\n\
             printf '%s\\n' \"$@\" >> \"{log}\"\n\
             case \"$1\" in\n\
             image) exit {inspect_exit} ;;\n\
             build) printf '%s' {stderr} >&2; exit {build_exit} ;;\n\
             *) exit 0 ;;\n\
             esac\n",
            log = argv_log.display(),
            stderr = shell_single_quote(build_stderr),
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

    fn docker_str(bin: &Path) -> String {
        bin.to_str().unwrap().to_string()
    }

    #[test]
    fn present_image_skips_build() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), 0, 0, "");
        let sandbox_root = tmp.path().join("sandbox");

        let tag = ensure_image(&docker_str(&docker), &sandbox_root).unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
        assert!(
            build_argv(&argv_log).is_empty(),
            "aucun build ne doit être lancé quand l'image est présente"
        );
    }

    #[test]
    fn absent_image_builds_then_returns_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), 1, 0, "");
        let sandbox_root = tmp.path().join("sandbox");

        let tag = ensure_image(&docker_str(&docker), &sandbox_root).unwrap();

        assert_eq!(tag, local_image_ref(EMBEDDED_DOCKERFILE.as_bytes()));
        assert_eq!(
            build_argv(&argv_log),
            vec![
                "build".to_string(),
                "-t".to_string(),
                tag.clone(),
                "-f".to_string(),
                dockerfile_path(&sandbox_root).display().to_string(),
                build_context_dir(&sandbox_root).display().to_string(),
            ]
        );
    }

    #[test]
    fn build_failure_is_explicit_error() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, _) = write_fake_docker(tmp.path(), 1, 1, "boom: base image missing");
        let sandbox_root = tmp.path().join("sandbox");

        let err = ensure_image(&docker_str(&docker), &sandbox_root).unwrap_err();

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

        let err = ensure_image(&docker_str(&missing), &sandbox_root).unwrap_err();

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
        let (docker, _) = write_fake_docker(tmp.path(), 1, 0, "");
        let sandbox_root = tmp.path().join("sandbox");
        assert!(!dockerfile_path(&sandbox_root).exists());

        ensure_image(&docker_str(&docker), &sandbox_root).unwrap();

        let seeded = std::fs::read(dockerfile_path(&sandbox_root)).unwrap();
        assert_eq!(
            seeded,
            EMBEDDED_DOCKERFILE.as_bytes(),
            "le Dockerfile seedé doit être identique à l'embarqué"
        );
    }

    #[test]
    fn edited_on_disk_dockerfile_is_preserved_and_drives_tag() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, argv_log) = write_fake_docker(tmp.path(), 1, 0, "");
        let sandbox_root = tmp.path().join("sandbox");
        // Pré-écrire un Dockerfile ÉDITÉ (différent de l'embarqué).
        std::fs::create_dir_all(&sandbox_root).unwrap();
        let edited: &[u8] = b"FROM ubuntu:24.04\nRUN echo edited\n";
        std::fs::write(dockerfile_path(&sandbox_root), edited).unwrap();

        let tag = ensure_image(&docker_str(&docker), &sandbox_root).unwrap();

        // (a) Octets inchangés : pas d'écrasement.
        assert_eq!(
            std::fs::read(dockerfile_path(&sandbox_root)).unwrap(),
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
        let (docker, argv_log) = write_fake_docker(tmp.path(), 1, 0, "");
        let sandbox_root = tmp.path().join("sandbox");

        ensure_image(&docker_str(&docker), &sandbox_root).unwrap();

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
        assert!(h.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()));
        assert!(local_image_ref(base).starts_with("pdo-sandbox:h-"));

        // GARDE-FOU PARITÉ CI : figer l'algo canonique. Épingle la sortie Rust au préfixe que
        // `release.yml`/#411 produiront en bash :
        //   printf 'FROM ubuntu:24.04\nRUN apt-get update\n' | sha256sum | cut -c1-12
        assert_eq!(h, "5804eefb8f92");
    }
}
