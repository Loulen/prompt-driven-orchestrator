//! Garantir l'image sandbox d'un Run (#405 build local, #411 pull GHCR hybride, #467 ref
//! explicite) : les effets docker du vocabulaire de [`crate::domain::image`], à travers le port
//! [`DockerCli`], et les fichiers à travers [`DockerfileStore`].
//!
//! **Sync délibéré (D3)** : `docker build`/`docker pull` sont bloquants et longs, leur place est
//! dans le `spawn_blocking` du caller async (#407), pas dans une tâche tokio.

use std::path::Path;

use anyhow::{Context, Result};

use super::ports::{DockerCli, DockerOutput, DockerfileStore};
use crate::domain::image::{
    local_image_ref, registry_image_ref, DockerProbe, DockerfileSource, ImagePlan, ImageSource,
    ResolvedDockerfile, DOCKERFILE_PATH_ENV, DOCKER_DAEMON_UNREACHABLE_MSG, DOCKER_NOT_FOUND_MSG,
};

/// `docker <args>`, the argv given as string slices.
pub(crate) fn docker(cli: &dyn DockerCli, args: &[&str]) -> std::io::Result<DockerOutput> {
    let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    cli.run(&args)
}

/// Probe whether this host can launch a sandboxed Run, by forcing a round-trip to the
/// Docker daemon: `docker version --format '{{.Server.Version}}'` (exit 0 ⇔ the daemon
/// answered). Three signals collapse to two states: spawn `NotFound` → not installed
/// (`DOCKER_NOT_FOUND_MSG`); any other spawn error or a non-zero exit → daemon
/// unreachable (`DOCKER_DAEMON_UNREACHABLE_MSG`); exit 0 → available.
///
/// Blocking, like every call through the port: the caller owns the timeout at the edge
/// (`spawn_blocking` + `tokio::time::timeout`, never the runtime thread — see
/// [`super::docker_probe`]).
/// Rejected alternatives (see plan D2): "binary on PATH" alone misses a stopped daemon
/// — precisely the case to gray out; an `ensure_image` dry-run costs minutes + network.
pub fn probe_docker(cli: &dyn DockerCli) -> DockerProbe {
    match docker(cli, &["version", "--format", "{{.Server.Version}}"]) {
        Ok(o) if o.success => DockerProbe {
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

// -- effets docker (bloquants, à travers le port `DockerCli`) ---------------

/// `docker image inspect <tag>` (métadonnée locale, jamais de réseau) : `Ok(true)` si exit 0
/// (présente), `Ok(false)` si exit != 0 (absente). `docker` introuvable (spawn `NotFound`) ->
/// `Err` explicite préservant l'`io::Error` en source (chaîne à 2 maillons, cf. #298).
pub(crate) fn image_exists(cli: &dyn DockerCli, tag: &str) -> Result<bool> {
    match docker(cli, &["image", "inspect", tag]) {
        Ok(output) => Ok(output.success),
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
    cli: &dyn DockerCli,
    tag: &str,
    dockerfile: &Path,
    context_dir: &Path,
) -> Result<()> {
    let args = [
        "build".to_string(),
        "-t".to_string(),
        tag.to_string(),
        "-f".to_string(),
        dockerfile.display().to_string(),
        context_dir.display().to_string(),
    ];
    let output = match cli.run(&args) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker build` while building the sandbox image");
        }
    };
    if !output.success {
        let stderr = &output.stderr;
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
pub(crate) fn pull_image(cli: &dyn DockerCli, registry_ref: &str) -> Result<PullOutcome> {
    match docker(cli, &["pull", registry_ref]) {
        Ok(output) => Ok(PullOutcome {
            ok: output.success,
            stderr: output.stderr,
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG)
        }
        Err(e) => Err(e).context("failed to run `docker pull` while fetching the sandbox image"),
    }
}

/// `docker tag <src> <dst>` : retague l'image tirée sous le ref local content-addressé, pour que
/// [`crate::domain::container`] reçoive TOUJOURS `pdo-sandbox:h-<hash>` (pull ou build → même nom).
/// Idempotent. Non-zéro → bail avec stderr (reason actionnable d'un `RunFailed`, US-16), miroir de
/// [`build_image`]. `docker` introuvable → `Err` explicite (comme [`pull_image`]).
pub(crate) fn tag_image(cli: &dyn DockerCli, src: &str, dst: &str) -> Result<()> {
    let output = match docker(cli, &["tag", src, dst]) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => return Err(e).context("failed to run `docker tag` for the sandbox image"),
    };
    if !output.success {
        let stderr = &output.stderr;
        anyhow::bail!(
            "failed to retag the pulled sandbox image `{src}` as `{dst}` — \
             `docker tag` exited with {}: {stderr}",
            output.status
        );
    }
    Ok(())
}

/// Provisionneur idempotent (**seul point d'entrée** de #406/#407) : garantit que l'image du Run
/// existe localement et retourne le ref que le conteneur doit poser au `docker create`.
///
/// Aiguillage à deux branches sur le plan résolu au bord (#467) — voir [`ImagePlan`] :
/// [`ensure_hash_derived_image`] (le chemin historique, adressé par contenu) ou
/// [`ensure_explicit_ref`] (un ref registry libre posé par un profil : pull-ou-échec, jamais de
/// build). C'est ici, et nulle part ailleurs, que la différence se décide.
///
/// **Sync délibéré (D3)** : `docker build`/`docker pull` sont bloquants et longs, leur place est
/// dans le `spawn_blocking` du caller async (#407), pas dans une tâche tokio ; garder ce module
/// sync laisse aussi les tests en `#[test]` simples.
pub fn ensure_image(
    cli: &dyn DockerCli,
    files: &dyn DockerfileStore,
    sandbox_root: &Path,
    plan: &ImagePlan,
) -> Result<String> {
    match plan {
        ImagePlan::HashDerived { dockerfile, source } => {
            ensure_hash_derived_image(cli, files, sandbox_root, dockerfile, *source)
        }
        ImagePlan::ExplicitRef { image_ref, profile } => {
            ensure_explicit_ref(cli, image_ref, profile)
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
pub fn ensure_explicit_ref(cli: &dyn DockerCli, image_ref: &str, profile: &str) -> Result<String> {
    if image_exists(cli, image_ref)? {
        return Ok(image_ref.to_string());
    }
    let pull = pull_image(cli, image_ref)?;
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
pub fn ensure_hash_derived_image(
    cli: &dyn DockerCli,
    files: &dyn DockerfileStore,
    sandbox_root: &Path,
    dockerfile: &ResolvedDockerfile,
    source: ImageSource,
) -> Result<String> {
    // 1. Seed le Dockerfile PAR DÉFAUT si absent — TOUJOURS, même sous un chemin custom :
    //    c'est la copie de référence que l'utilisateur édite et la matérialisation du tier
    //    `default` pour quand le réglage est effacé. JAMAIS écrit à un chemin custom.
    files.seed_default(sandbox_root)?;
    // 2. Le Dockerfile RÉSOLU (#431). Chemin absent / non-fichier-régulier = erreur DURE
    //    nommant chemin + tier : jamais de repli silencieux vers le seedé (ADR-0030 pt 4,
    //    amendement #431). `is_file()` et NON `exists()` — `exists()` est VRAI pour un
    //    répertoire, et `is_file()` suit correctement un Dockerfile symlinké tout en
    //    rejetant un lien pendant. Ce contrôle DOIT précéder le fast-path `image_exists` :
    //    le tag EST le hash de ces octets, donc sans eux l'image est innommable.
    let path = dockerfile.path.as_path();
    if !files.is_file(path) {
        // La remédiation dépend du tier gagnant : dire « édite le profil » à qui a posé
        // `PDO_SANDBOX_DOCKERFILE` dans l'env du daemon l'enverrait au mauvais endroit, et
        // réciproquement. Le tier et le chemin, eux, sont nommés dans les trois cas. Le tier
        // `default` ne peut pointer que le Dockerfile seedé, que `seed_default` vient
        // d'écrire quelques lignes plus haut : y arriver signifie que quelque chose l'a
        // supprimé entre-temps (#471 — plus aucun réglage stocké ne peut pointer ailleurs).
        let seeded = crate::domain::image::default_dockerfile_path(sandbox_root)
            .display()
            .to_string();
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
    let bytes = files
        .read(path)
        .with_context(|| format!("failed to read sandbox Dockerfile at {}", path.display()))?;
    // 4. Ref local content-addressé = TOUJOURS la valeur de retour (invariant sandbox_container).
    //    Le NOM vient du fichier résolu (#466 : `Dockerfile.chrome-dev` → `pdo-sandbox-chrome-dev`),
    //    le TAG de ses octets — deux variantes ne partagent donc ni nom ni tag.
    let local_ref = local_image_ref(&dockerfile.image_name, &bytes);
    // 5. FAST PATH — précède TOUT réseau : image déjà locale → ni pull ni build (offline-safe).
    if image_exists(cli, &local_ref)? {
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
        if pull_image(cli, &registry_ref)?.ok {
            tag_image(cli, &registry_ref, &local_ref)?;
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
    let context_dir = files.ensure_build_context(sandbox_root).with_context(|| {
        format!(
            "failed to create sandbox build context at {}",
            crate::domain::image::build_context_dir(sandbox_root).display()
        )
    })?;
    build_image(cli, &local_ref, path, &context_dir)?;
    Ok(local_ref)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::docker_cli::ProcessDocker;
    use crate::adapters::dockerfiles::{FsDockerfiles, EMBEDDED_DOCKERFILE};
    use crate::domain::image::{
        build_context_dir, default_dockerfile_path, dockerfile_hash, resolve_dockerfile,
        BASE_IMAGE_NAME,
    };
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    // The cases below drive the use cases through the real adapters: a fake `docker`
    // executable (a script, threaded as the binary) and the real filesystem. The
    // in-memory cases at the bottom pin the orchestration alone.

    fn ensure_hash_derived_image(
        docker_bin: &str,
        sandbox_root: &Path,
        dockerfile: &ResolvedDockerfile,
        source: ImageSource,
    ) -> Result<String> {
        super::ensure_hash_derived_image(
            &ProcessDocker::new(docker_bin),
            &FsDockerfiles,
            sandbox_root,
            dockerfile,
            source,
        )
    }

    fn ensure_image(docker_bin: &str, sandbox_root: &Path, plan: &ImagePlan) -> Result<String> {
        super::ensure_image(
            &ProcessDocker::new(docker_bin),
            &FsDockerfiles,
            sandbox_root,
            plan,
        )
    }

    fn probe_docker(docker_bin: &str) -> DockerProbe {
        super::probe_docker(&ProcessDocker::new(docker_bin))
    }

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

        assert_eq!(
            std::fs::read(default_dockerfile_path(&sandbox_root)).unwrap(),
            edited,
            "le seed ne doit jamais écraser un Dockerfile existant"
        );
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
        assert_eq!(
            invocation(&argv_log, "pull"),
            Some(vec!["pull".to_string(), registry_ref.clone()]),
            "pull must target the registry ref"
        );
        assert_eq!(
            invocation(&argv_log, "tag"),
            Some(vec![
                "tag".to_string(),
                registry_ref.clone(),
                local_ref.clone()
            ]),
            "tag must retag the pulled ref under the local ref"
        );
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

    fn explicit(image_ref: &str) -> ImagePlan {
        ImagePlan::ExplicitRef {
            image_ref: image_ref.to_string(),
            profile: "chrome".to_string(),
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

    // -- Étage 2 : l'orchestration seule, docker et disque en mémoire ------------

    use crate::application::testing::{InMemoryDockerfiles, ScriptedDocker};

    #[test]
    fn a_local_image_is_reused_without_network_or_build() {
        let docker = ScriptedDocker::answering(|args| match args[0].as_str() {
            "image" => ScriptedDocker::ok(""),
            _ => ScriptedDocker::fail(1, "unexpected"),
        });
        let files = InMemoryDockerfiles::seeded(b"FROM scratch\n");
        let root = Path::new("/sb");
        let plan = ImagePlan::HashDerived {
            dockerfile: resolve_dockerfile(None, None, root),
            source: ImageSource::Registry,
        };
        let tag = super::ensure_image(&docker, &files, root, &plan).unwrap();
        assert_eq!(tag, local_image_ref(BASE_IMAGE_NAME, b"FROM scratch\n"));
        assert_eq!(docker.verbs(), vec!["image"]);
    }

    #[test]
    fn a_failed_pull_of_the_seeded_dockerfile_falls_back_to_a_build() {
        let docker = ScriptedDocker::answering(|args| match args[0].as_str() {
            "image" | "pull" => ScriptedDocker::fail(1, "offline"),
            _ => ScriptedDocker::ok(""),
        });
        let files = InMemoryDockerfiles::seeded(b"FROM scratch\n");
        let root = Path::new("/sb");
        let plan = ImagePlan::HashDerived {
            dockerfile: resolve_dockerfile(None, None, root),
            source: ImageSource::Registry,
        };
        super::ensure_image(&docker, &files, root, &plan).unwrap();
        assert_eq!(docker.verbs(), vec!["image", "pull", "build"]);
        assert_eq!(files.build_contexts(), vec![build_context_dir(root)]);
    }

    #[test]
    fn an_explicit_ref_never_touches_the_dockerfiles() {
        let docker = ScriptedDocker::answering(|args| match args[0].as_str() {
            "image" => ScriptedDocker::fail(1, ""),
            _ => ScriptedDocker::ok(""),
        });
        let files = InMemoryDockerfiles::seeded(b"FROM scratch\n");
        let plan = ImagePlan::ExplicitRef {
            image_ref: "acme/agent:1".into(),
            profile: "team".into(),
        };
        let tag = super::ensure_image(&docker, &files, Path::new("/sb"), &plan).unwrap();
        assert_eq!(tag, "acme/agent:1");
        assert_eq!(docker.verbs(), vec!["image", "pull"]);
        assert!(files.seeded_roots().is_empty() && files.build_contexts().is_empty());
    }

    #[test]
    fn docker_absent_is_the_one_canonical_message() {
        let docker = ScriptedDocker::absent();
        assert_eq!(
            super::probe_docker(&docker).reason.as_deref(),
            Some(DOCKER_NOT_FOUND_MSG)
        );
        let err = image_exists(&docker, "x").unwrap_err();
        assert!(format!("{err:#}").contains("not found on PATH"));
    }
}
