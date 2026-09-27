//! Cycle de vie du conteneur sandbox d'un Run (#406) : sonde, création, démarrage,
//! identité, kill ciblé, sonde de binaire, destruction — les argv purs de
//! [`crate::domain::container`], exécutés à travers le port [`DockerCli`].

use std::path::Path;

use anyhow::{Context, Result};
use tracing::warn;

use super::image::docker;
use super::ports::DockerCli;
use crate::domain::container::{
    container_name, create_args, identity_exec_args, is_absent_stderr, is_name_conflict_stderr,
    kill_one_liner, sh_single_quote, ContainerSpec,
};
use crate::domain::image::DOCKER_NOT_FOUND_MSG;

/// État du conteneur tel que résolu par la sonde. Privé : le monde extérieur voit
/// [`ensure_running`], pas les états intermédiaires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContainerState {
    Running,
    Stopped,
    Absent,
}

/// `docker container inspect -f '{{.State.Running}}' <name>` (sonde vérifiée sur Docker 29.2.1) :
/// - exit 0 + stdout `true`  → [`ContainerState::Running`] ;
/// - exit 0 + stdout `false` → [`ContainerState::Stopped`] ;
/// - exit != 0 **et** stderr sentinelle `no such container`/`no such object` → [`ContainerState::Absent`] ;
/// - exit != 0 avec TOUT AUTRE stderr, **ou** exit 0 avec un stdout inattendu, **ou** spawn
///   `NotFound` → **`Err`** (jamais Absent : une erreur transitoire — daemon Docker down,
///   permission — ne doit pas déclencher un `docker create` qui masquerait le vrai problème).
fn probe_state(cli: &dyn DockerCli, name: &str) -> Result<ContainerState> {
    let output = match docker(
        cli,
        &["container", "inspect", "-f", "{{.State.Running}}", name],
    ) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context(
                "failed to run `docker container inspect` while probing the sandbox container",
            );
        }
    };

    if output.success {
        match output.stdout.trim() {
            "true" => Ok(ContainerState::Running),
            "false" => Ok(ContainerState::Stopped),
            other => anyhow::bail!(
                "unexpected `docker container inspect` output while probing `{name}`: {other:?} \
                 (expected `true`/`false`) — refusing to treat as an absent container"
            ),
        }
    } else {
        let stderr = &output.stderr;
        if is_absent_stderr(stderr) {
            Ok(ContainerState::Absent)
        } else {
            anyhow::bail!(
                "`docker container inspect {name}` failed transiently ({}): {stderr} \
                 — refusing to treat a transient docker error as an absent container",
                output.status
            )
        }
    }
}

/// Provisionneur IDEMPOTENT du conteneur (miroir de [`super::image::ensure_image`]) :
/// sonde → up → no-op ; arrêté → `start` ; absent → `create` + `start`. Toute erreur de sonde
/// (transitoire, docker absent) remonte via `?` sans jamais être traitée comme une absence.
///
/// **`spec` n'est consulté que sur le bras `Absent`.** `docker start` ne **réévalue jamais** les
/// mounts d'un conteneur pré-existant : ils sont figés à `docker create`. Cohérent avec le gel
/// d'ADR-0031 §6 — éditer un profil en cours de Run ne remonte rien, et ne doit rien remonter — mais
/// jusqu'ici non dit, alors que la queue variable de [`ContainerSpec::extra_mounts`] (#432) rend la
/// question naturelle.
pub fn ensure_running(cli: &dyn DockerCli, run_id: &str, spec: &ContainerSpec) -> Result<()> {
    let name = container_name(run_id);
    match probe_state(cli, &name)? {
        ContainerState::Running => {}
        ContainerState::Stopped => start_container(cli, &name)?,
        ContainerState::Absent => {
            create_container(cli, run_id, spec)?;
            start_container(cli, &name)?;
        }
    }
    // #414 : l'identité se pose sur le conteneur DÉMARRÉ, pas dans l'argv du create — voir
    // `identity_script`. Sur les trois branches : un conteneur d'un daemon antérieur rattrape.
    ensure_identity(cli, run_id, spec.uid, spec.gid, spec.host_home);
    Ok(())
}

/// `docker` + [`create_args`]. `create` et `start` sont DEUX primitives (pas `run -d`) : la
/// branche Stopped a de toute façon besoin de `start`, et un conteneur arrêté ne se `run` pas à
/// nouveau. Pas de `--restart` (PDO possède le cycle de vie ; `unless-stopped` ressusciterait des
/// conteneurs que PDO croit finis → fuite). Course au create (v1, mono-appelant par `run_id`) :
/// un stderr `Conflict … already in use` est mappé sur un **succès bénin** (le `start` suivant est
/// idempotent).
fn create_container(cli: &dyn DockerCli, run_id: &str, spec: &ContainerSpec) -> Result<()> {
    let args = create_args(run_id, spec);
    let output = match cli.run(&args) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker create` for the sandbox container");
        }
    };
    if output.success {
        return Ok(());
    }
    let stderr = &output.stderr;
    if is_name_conflict_stderr(stderr) {
        return Ok(()); // course bénigne : le conteneur existe déjà, on enchaîne `start`.
    }
    anyhow::bail!(
        "failed to create the sandbox container `{}` — `docker create` exited with {}: {stderr}",
        container_name(run_id),
        output.status
    )
}

/// `docker start <name>` (`docker start` ne prend pas `-d`). Non-zéro → bail avec le stderr docker.
fn start_container(cli: &dyn DockerCli, name: &str) -> Result<()> {
    let output = match docker(cli, &["start", name]) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker start` for the sandbox container");
        }
    };
    if output.success {
        Ok(())
    } else {
        let stderr = &output.stderr;
        anyhow::bail!(
            "failed to start the sandbox container `{name}` — `docker start` exited with {}: {stderr}",
            output.status
        )
    }
}

/// Pose l'identité de l'uid/gid hôte dans le conteneur ([`identity_script`]). Appelée par
/// [`ensure_running`] sur les **trois** branches, après le `start`.
///
/// **Best-effort, par choix** : la grande majorité des Runs n'invoquent jamais `sudo`. Faire
/// échouer un Run parce qu'une commodité n'a pas pu être posée serait pire que le défaut
/// corrigé — donc `warn!` et on continue, jamais de `?` vers l'appelant (miroir de
/// `prepare::kill_session_best_effort`). C'est l'exact opposé de la politique des autonomy fixups du
/// staging set, et pour une raison nette : un fixup désarme des dialogues qui **bloquent** un
/// agent non surveillé, ici l'agent voit juste une commande échouer.
///
/// Sur les trois branches et pas seulement `Absent` : un conteneur créé par un daemon antérieur
/// et toujours vivant doit rattraper son identité. La garde `getent` rend les appels répétés
/// gratuits.
fn ensure_identity(cli: &dyn DockerCli, run_id: &str, uid: u32, gid: u32, host_home: &Path) {
    let name = container_name(run_id);
    let args = identity_exec_args(run_id, uid, gid, host_home);
    match cli.run(&args) {
        Ok(output) if output.success => {}
        Ok(output) => {
            let stderr = &output.stderr;
            if is_absent_stderr(stderr) {
                return; // le conteneur a disparu entre le start et ici — rien à identifier.
            }
            warn!(
                "sandbox: could not give uid {uid} a passwd entry in `{name}` \
                 (`docker exec` exited with {}: {stderr}) — `sudo` will refuse to start inside \
                 the container",
                output.status
            );
        }
        Err(e) => {
            warn!("sandbox: could not run `docker exec` to identify uid {uid} in `{name}`: {e:#}");
        }
    }
}

/// Kill CIBLÉ : un `docker exec` SÉPARÉ qui scanne `/proc/*/environ` pour le marqueur puis
/// `TERM` → `sleep 2` → `KILL`. Nécessaire car le client `docker exec` tué côté tmux ne tue pas
/// le process conteneur (reparenté sur PID 1) ; les sessions SŒURS du même Run survivent.
///
/// **Aucun `-e SESSION_MARKER_ENV` sur ce kill exec** (sinon `sh`/`grep` se matcheraient
/// eux-mêmes). Réutilise le même `--user <uid>:<gid>` (règle same-uid : un uid ne peut
/// signaler / lire `/proc/environ` que de ses propres process). Best-effort : conteneur déjà
/// absent (sentinelle) → `Ok`.
pub fn kill_session_in_container(
    cli: &dyn DockerCli,
    run_id: &str,
    marker: &str,
    uid: u32,
    gid: u32,
) -> Result<()> {
    let name = container_name(run_id);
    let user = format!("{uid}:{gid}");
    let script = kill_one_liner(marker);
    let output = match docker(cli, &["exec", "--user", &user, &name, "sh", "-c", &script]) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker exec` to kill a sandbox session");
        }
    };
    if output.success {
        return Ok(());
    }
    let stderr = &output.stderr;
    if is_absent_stderr(stderr) {
        return Ok(()); // best-effort : plus de conteneur, plus rien à tuer.
    }
    anyhow::bail!(
        "failed to kill session `{marker}` in sandbox container `{name}` — \
         `docker exec` exited with {}: {stderr}",
        output.status
    )
}

/// Le binaire `binary` est-il résolvable **dans le conteneur** du Run (#708, ADR-0063 §5) ?
/// Un `docker exec --user <uid>:<gid> <name> bash -lc 'command -v -- <binary>'` — le
/// **même** shell de login que la tail de la session, donc le même `PATH` qu'elle
/// verra. `Ok(true)` / `Ok(false)` sur un verdict du shell ; `Err` quand `docker`
/// lui-même n'a pas pu rendre de verdict (binaire absent, conteneur disparu) — que
/// l'appelant distingue : une sonde qui n'a pas répondu n'est pas un binaire absent.
///
/// **Jamais** une validation d'image : un binaire présent mais d'une version non
/// validée passe, comme sur l'hôte (ADR-0063 « limites »). Et pas de `--version` : un
/// `which` n'exécute pas le harnais.
pub fn binary_present_in_container(
    cli: &dyn DockerCli,
    run_id: &str,
    uid: u32,
    gid: u32,
    binary: &str,
) -> Result<bool> {
    let name = container_name(run_id);
    let user = format!("{uid}:{gid}");
    let script = format!("command -v -- {} >/dev/null 2>&1", sh_single_quote(binary));
    let output = match docker(
        cli,
        &["exec", "--user", &user, &name, "bash", "-lc", &script],
    ) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker exec` to probe a harness binary");
        }
    };
    if output.success {
        return Ok(true);
    }
    let stderr = &output.stderr;
    if is_absent_stderr(stderr) || stderr.contains("is not running") {
        anyhow::bail!(
            "sandbox container `{name}` cannot answer a binary probe — `docker exec` exited \
             with {}: {stderr}",
            output.status
        );
    }
    // `command -v` exits 1 (or 127 from a `bash -lc` that could not find it) when
    // the name does not resolve; any other shell verdict is read the same way — the
    // only thing that says "present" is a zero exit.
    Ok(false)
}

/// `docker rm -f <name>` au `cleanup_run`. Idempotent : conteneur déjà absent (sentinelle) →
/// `Ok`. C'est LE verbe « destroy / destruction » du conteneur (distinct de
/// [`kill_session_in_container`], qui ne tue qu'un arbre de session).
pub fn remove(cli: &dyn DockerCli, run_id: &str) -> Result<()> {
    let name = container_name(run_id);
    let output = match docker(cli, &["rm", "-f", &name]) {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker rm -f` for the sandbox container");
        }
    };
    if output.success {
        return Ok(());
    }
    let stderr = &output.stderr;
    if is_absent_stderr(stderr) {
        return Ok(()); // idempotent : déjà supprimé.
    }
    anyhow::bail!(
        "failed to remove the sandbox container `{name}` — `docker rm -f` exited with {}: {stderr}",
        output.status
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::docker_cli::ProcessDocker;
    use crate::application::testing::ScriptedDocker;
    use crate::domain::staging::StagedMount;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    // The cases below drive the use cases through the real CLI adapter, against a fake
    // `docker` executable threaded as the binary; the in-memory cases at the bottom pin
    // the orchestration alone.

    fn ensure_running(docker_bin: &str, run_id: &str, spec: &ContainerSpec) -> Result<()> {
        super::ensure_running(&ProcessDocker::new(docker_bin), run_id, spec)
    }

    fn kill_session_in_container(
        docker_bin: &str,
        run_id: &str,
        marker: &str,
        uid: u32,
        gid: u32,
    ) -> Result<()> {
        super::kill_session_in_container(&ProcessDocker::new(docker_bin), run_id, marker, uid, gid)
    }

    fn remove(docker_bin: &str, run_id: &str) -> Result<()> {
        super::remove(&ProcessDocker::new(docker_bin), run_id)
    }

    fn binary_present_in_container(
        docker_bin: &str,
        run_id: &str,
        uid: u32,
        gid: u32,
        binary: &str,
    ) -> Result<bool> {
        super::binary_present_in_container(
            &ProcessDocker::new(docker_bin),
            run_id,
            uid,
            gid,
            binary,
        )
    }

    /// Single-quote pour l'embarquement dans le script bash du fake docker (aucune mutation
    /// d'`std::env` : le fake est threadé comme `docker_bin`, cf. discipline #405 parallèle-cargo).
    fn q(s: &str) -> String {
        format!("'{}'", s.replace('\'', "'\\''"))
    }

    /// Comportement injecté du fake docker, par sous-commande. `Default` = tout exit 0, inspect
    /// répond `true` (conteneur up).
    #[derive(Clone)]
    struct FakeSpec {
        inspect_exit: i32,
        inspect_stdout: String,
        inspect_stderr: String,
        create_exit: i32,
        create_stderr: String,
        start_exit: i32,
        exec_exit: i32,
        exec_stderr: String,
        rm_exit: i32,
        rm_stderr: String,
    }

    impl Default for FakeSpec {
        fn default() -> Self {
            Self {
                inspect_exit: 0,
                inspect_stdout: "true".to_string(),
                inspect_stderr: String::new(),
                create_exit: 0,
                create_stderr: String::new(),
                start_exit: 0,
                exec_exit: 0,
                exec_stderr: String::new(),
                rm_exit: 0,
                rm_stderr: String::new(),
            }
        }
    }

    /// Écrit un faux `docker` exécutable et renvoie `(docker_bin, argv_log)`. Branche sur `$1`
    /// (`container`→inspect, `create`, `start`, `exec`, `rm`) et NON sur `"$1 $2"` : un vrai
    /// `docker container inspect …` a `$2 = "inspect"`. Chaque invocation append son argv (une
    /// ligne par arg) dans `argv.log`. Aucune mutation d'`std::env`.
    fn write_fake_docker(dir: &Path, spec: &FakeSpec) -> (String, PathBuf) {
        let bin = dir.join("fake-docker");
        let log = dir.join("argv.log");
        let script = format!(
            "#!/usr/bin/env bash\n\
             printf '%s\\n' \"$@\" >> {log}\n\
             case \"$1\" in\n\
             container) printf '%s' {ins_out}; printf '%s' {ins_err} >&2; exit {ins_exit} ;;\n\
             create) printf '%s' {cr_err} >&2; exit {cr_exit} ;;\n\
             start) exit {st_exit} ;;\n\
             exec) printf '%s' {ex_err} >&2; exit {ex_exit} ;;\n\
             rm) printf '%s' {rm_err} >&2; exit {rm_exit} ;;\n\
             *) exit 0 ;;\n\
             esac\n",
            log = q(&log.display().to_string()),
            ins_out = q(&spec.inspect_stdout),
            ins_err = q(&spec.inspect_stderr),
            ins_exit = spec.inspect_exit,
            cr_err = q(&spec.create_stderr),
            cr_exit = spec.create_exit,
            st_exit = spec.start_exit,
            ex_err = q(&spec.exec_stderr),
            ex_exit = spec.exec_exit,
            rm_err = q(&spec.rm_stderr),
            rm_exit = spec.rm_exit,
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        (bin.to_str().unwrap().to_string(), log)
    }

    fn log_lines(log: &Path) -> Vec<String> {
        std::fs::read_to_string(log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// Fixtures possédant les chemins pointés par [`ContainerSpec`] (dont les refs empruntent).
    struct Fixtures {
        repo: PathBuf,
        run_wt: PathBuf,
        home: PathBuf,
        json: PathBuf,
        pdo: PathBuf,
        host_home: PathBuf,
        image: String,
        /// Queue variable (#432). Vide par défaut — c'est ce qui garde l'argv
        /// byte-identique à #406 pour un profil sans entrée d'exception `$HOME`.
        extras: Vec<StagedMount>,
        /// Racines de home des harnais first-party hors `.claude` (#708). Par
        /// défaut celle de `pi`, comme en production (le registre embarqué la porte).
        harness_homes: Vec<StagedMount>,
        /// Queue variable (#468). Vide par défaut, pour la même raison.
        env: BTreeMap<String, String>,
        /// Queue variable (ADR-0047). Vide par défaut — un Run sans secondaire
        /// modifiable rend un argv byte-identique.
        writable_gitdirs: Vec<PathBuf>,
    }

    impl Fixtures {
        fn sample() -> Self {
            Self {
                repo: PathBuf::from("/repo"),
                run_wt: PathBuf::from("/repo/.pdo/runs/r1/worktree"),
                home: PathBuf::from("/sb/r1/claude-home"),
                json: PathBuf::from("/sb/r1/.claude.json"),
                pdo: PathBuf::from("/host/bin/pdo"),
                host_home: PathBuf::from("/home/u"),
                image: "pdo-sandbox:h-abc123".to_string(),
                extras: Vec::new(),
                harness_homes: vec![StagedMount {
                    source: PathBuf::from("/sb/r1/home/.pi/agent"),
                    target: PathBuf::from("/home/u/.pi/agent"),
                }],
                env: BTreeMap::new(),
                writable_gitdirs: Vec::new(),
            }
        }

        fn spec(&self) -> ContainerSpec<'_> {
            ContainerSpec {
                image_ref: &self.image,
                repo_root: &self.repo,
                run_worktree: &self.run_wt,
                staged_home: &self.home,
                staged_json: &self.json,
                pdo_bin: &self.pdo,
                host_home: &self.host_home,
                uid: 1000,
                gid: 1000,
                daemon_port: 6172,
                extra_mounts: &self.extras,
                harness_homes: &self.harness_homes,
                env: &self.env,
                writable_secondary_gitdirs: &self.writable_gitdirs,
            }
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Sous `cargo test --workspace`, exec-er un binaire **fraîchement écrit** peut renvoyer
    /// `ETXTBSY` (os error 26) : un autre test qui `fork`+`exec` au même instant hérite
    /// transitoirement du fd d'écriture du fake docker (race Rust bien connue, rust-lang/rust#45719).
    /// La prod n'exec JAMAIS un binaire fraîchement écrit (`docker` est stable) → le retry vit
    /// ICI, pas dans le cœur. Une tentative ETXTBSY échoue **avant** que le script ne tourne, donc
    /// elle n'écrit rien dans `argv.log` : les assertions d'argv restent exactes après retry.
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

    /// Position de l'`exec` d'identité (#414) dans l'`argv.log` du fake docker : la ligne
    /// `0:0` en est le témoin UNIQUE (aucune autre invocation ne porte ce `--user`).
    fn identity_exec_at(lines: &[String]) -> Option<usize> {
        let user_at = lines.iter().position(|l| l == "0:0")?;
        Some(user_at - 2) // `exec`, `--user`, `0:0`
    }

    /// L'argv complet de l'`exec` d'identité tel que le fake docker l'a vu.
    fn identity_exec_argv(lines: &[String]) -> Vec<String> {
        let at = identity_exec_at(lines).expect("l'exec d'identité doit avoir tourné");
        lines[at..(at + 7).min(lines.len())].to_vec()
    }

    #[test]
    fn running_reuses() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path(), &FakeSpec::default()); // inspect → true
        let fx = Fixtures::sample();

        retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap();

        let lines = log_lines(&log);
        assert!(
            lines.contains(&"container".to_string()) && lines.contains(&"inspect".to_string()),
            "la sonde inspect doit avoir tourné"
        );
        assert!(
            !lines.contains(&"create".to_string()),
            "running → pas de create"
        );
        assert!(
            !lines.contains(&"start".to_string()),
            "running → pas de start"
        );
        // #414 : un conteneur déjà up d'un daemon ANTÉRIEUR rattrape son identité — c'est
        // pour ça que l'injection vit après le `match`, pas dans le bras `Absent`.
        assert_eq!(
            identity_exec_argv(&lines),
            identity_exec_args("r1", 1000, 1000, Path::new("/home/u")),
            "running → l'identité est tout de même posée"
        );
    }

    #[test]
    fn stopped_starts_only() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            inspect_stdout: "false".to_string(),
            ..Default::default()
        };
        let (docker, log) = write_fake_docker(tmp.path(), &spec);
        let fx = Fixtures::sample();

        retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap();

        let lines = log_lines(&log);
        assert!(lines.contains(&"start".to_string()), "stopped → start");
        assert!(
            !lines.contains(&"create".to_string()),
            "stopped → pas de create"
        );
        // #414 : l'identité se pose sur un conteneur DÉMARRÉ — donc après le `start`.
        let start_pos = lines.iter().position(|l| l == "start").unwrap();
        assert!(
            identity_exec_at(&lines).unwrap() > start_pos,
            "l'exec d'identité doit suivre le start: {lines:?}"
        );
    }

    #[test]
    fn absent_creates_then_starts() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            inspect_exit: 1,
            inspect_stdout: String::new(),
            inspect_stderr: "Error: No such container: pdo-sbx-r1".to_string(),
            ..Default::default()
        };
        let (docker, log) = write_fake_docker(tmp.path(), &spec);
        let fx = Fixtures::sample();

        retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap();

        let lines = log_lines(&log);
        let create_pos = lines
            .iter()
            .position(|l| l == "create")
            .expect("create attendu");
        let start_pos = lines
            .iter()
            .position(|l| l == "start")
            .expect("start attendu");
        assert!(create_pos < start_pos, "create doit précéder start");
        assert!(
            lines.contains(&"sleep".to_string()) && lines.contains(&"infinity".to_string()),
            "create doit poser `sleep infinity`"
        );
        // #414 : create → start → identité, dans cet ordre. L'inverser exec-erait dans un
        // conteneur pas encore démarré.
        assert!(
            identity_exec_at(&lines).unwrap() > start_pos,
            "l'exec d'identité doit suivre le start: {lines:?}"
        );
    }

    /// #414 — l'identité est posée sur les **trois** branches de la machine à états, jamais
    /// sur la seule `Absent`. Un conteneur créé par un daemon antérieur à #414 et toujours
    /// vivant (branche `Running`) doit rattraper : c'est exactement le cas qu'une injection
    /// posée au `create` aurait laissé sans identité pour toute la durée du Run.
    #[test]
    fn identity_is_ensured_on_all_three_branches() {
        let branches = [
            ("running", FakeSpec::default()),
            (
                "stopped",
                FakeSpec {
                    inspect_stdout: "false".to_string(),
                    ..Default::default()
                },
            ),
            (
                "absent",
                FakeSpec {
                    inspect_exit: 1,
                    inspect_stdout: String::new(),
                    inspect_stderr: "Error: No such container: pdo-sbx-r1".to_string(),
                    ..Default::default()
                },
            ),
        ];
        for (label, spec) in branches {
            let tmp = tempfile::tempdir().unwrap();
            let (docker, log) = write_fake_docker(tmp.path(), &spec);
            let fx = Fixtures::sample();

            retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap();

            let lines = log_lines(&log);
            assert_eq!(
                identity_exec_argv(&lines),
                identity_exec_args("r1", 1000, 1000, Path::new("/home/u")),
                "branche `{label}` : l'identité doit être posée"
            );
        }
    }

    /// D8 — best-effort : un `docker exec` d'identité en échec `warn!` et laisse le Run
    /// continuer. La majorité des Runs n'invoquent jamais `sudo` ; faire échouer un Run parce
    /// qu'une commodité n'a pas pu être posée serait pire que le défaut corrigé.
    #[test]
    fn ensure_identity_never_fails_the_run() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            inspect_exit: 1,
            inspect_stdout: String::new(),
            inspect_stderr: "Error: No such container: pdo-sbx-r1".to_string(),
            exec_exit: 1,
            exec_stderr: "boom".to_string(),
            ..Default::default()
        };
        let (docker, log) = write_fake_docker(tmp.path(), &spec);
        let fx = Fixtures::sample();

        retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap();

        let lines = log_lines(&log);
        assert!(lines.contains(&"create".to_string()));
        assert!(
            identity_exec_at(&lines).is_some(),
            "l'exec a bien été tenté"
        );
    }

    #[test]
    fn transient_not_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            inspect_exit: 1,
            inspect_stdout: String::new(),
            inspect_stderr: "Cannot connect to the Docker daemon at unix:///var/run/docker.sock."
                .to_string(),
            ..Default::default()
        };
        let (docker, log) = write_fake_docker(tmp.path(), &spec);
        let fx = Fixtures::sample();

        let err = retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap_err();

        assert!(
            format!("{err:#}").contains("transient"),
            "une erreur docker transitoire doit être signalée comme telle: {err:#}"
        );
        assert!(
            !log_lines(&log).contains(&"create".to_string()),
            "erreur transitoire ≠ absent : jamais de create"
        );
    }

    #[test]
    fn unexpected_output_not_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            inspect_stdout: "garbage".to_string(),
            ..Default::default()
        };
        let (docker, log) = write_fake_docker(tmp.path(), &spec);
        let fx = Fixtures::sample();

        let err = retry_etxtbsy(|| ensure_running(&docker, "r1", &fx.spec())).unwrap_err();

        assert!(
            format!("{err:#}").contains("unexpected"),
            "un stdout inattendu doit être une erreur, jamais une absence: {err:#}"
        );
        assert!(!log_lines(&log).contains(&"create".to_string()));
    }

    #[test]
    fn docker_missing_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-docker");
        let fx = Fixtures::sample();

        let err = ensure_running(missing.to_str().unwrap(), "r1", &fx.spec()).unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("Docker") && msg.contains("not found on PATH"),
            "message docker-absent attendu: {msg}"
        );
        // Chaîne à 2 maillons : la source io::NotFound est préservée (#298).
        assert!(
            err.chain().count() >= 2,
            "la source io::Error doit être préservée dans la chaîne anyhow"
        );
        assert!(
            !tmp.path().join("argv.log").exists(),
            "docker absent → argv-log vide"
        );
    }

    #[test]
    fn kill_targets_only_marker() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path(), &FakeSpec::default()); // exec → exit 0

        retry_etxtbsy(|| kill_session_in_container(&docker, "r1", "pdo-r1-n1-iter-1", 1000, 1000))
            .unwrap();

        let lines = log_lines(&log);
        assert_eq!(
            lines[..6],
            strings(&["exec", "--user", "1000:1000", "pdo-sbx-r1", "sh", "-c"])[..],
            "argv du kill exec"
        );
        let one_liner = &lines[6];
        assert!(
            one_liner.contains("PDO_SBX_SESSION=pdo-r1-n1-iter-1"),
            "le one-liner doit cibler CE marqueur: {one_liner}"
        );
        assert!(
            !one_liner.contains("pdo-r1-n2-iter-1"),
            "le one-liner ne doit PAS cibler une session sœur"
        );
        // `grep -qazx` : match ligne entière (pas de faux positif sur un préfixe).
        assert!(
            one_liner.contains("grep -qazx"),
            "match exact NUL-record attendu"
        );
        // Pas de -e marqueur sur le kill exec (sinon sh/grep se matchent eux-mêmes).
        assert!(
            !lines.contains(&"-e".to_string()),
            "pas de -e SESSION_MARKER_ENV sur le kill exec"
        );
    }

    #[test]
    fn remove_present_ok() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path(), &FakeSpec::default()); // rm → exit 0

        retry_etxtbsy(|| remove(&docker, "r1")).unwrap();

        assert_eq!(log_lines(&log), strings(&["rm", "-f", "pdo-sbx-r1"]));
    }

    #[test]
    fn remove_absent_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            rm_exit: 1,
            rm_stderr: "Error: No such container: pdo-sbx-r1".to_string(),
            ..Default::default()
        };
        let (docker, _) = write_fake_docker(tmp.path(), &spec);

        retry_etxtbsy(|| remove(&docker, "r1")).unwrap();
    }

    #[test]
    fn remove_docker_missing_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-docker");

        let err = remove(missing.to_str().unwrap(), "r1").unwrap_err();

        let msg = format!("{err:#}");
        assert!(
            msg.contains("Docker") && msg.contains("not found on PATH"),
            "message docker-absent attendu: {msg}"
        );
        assert!(err.chain().count() >= 2, "source io::Error préservée");
    }

    #[test]
    fn binary_present_in_container_true_on_zero_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let (docker, log) = write_fake_docker(tmp.path(), &FakeSpec::default());
        assert!(
            retry_etxtbsy(|| binary_present_in_container(&docker, "r1", 1000, 1000, "pi")).unwrap()
        );
        // It really ran a `docker exec` (never the binary itself).
        assert!(log_lines(&log).contains(&"exec".to_string()));
    }

    /// A non-zero shell exit (the name did not resolve) is a clean `Ok(false)`, not an
    /// error — the caller turns it into a node `Interrupted`, said once per Run.
    #[test]
    fn binary_present_in_container_false_when_command_v_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            exec_exit: 1,
            ..FakeSpec::default()
        };
        let (docker, _log) = write_fake_docker(tmp.path(), &spec);
        assert!(
            !retry_etxtbsy(|| binary_present_in_container(&docker, "r1", 1000, 1000, "pi"))
                .unwrap()
        );
    }

    /// A container that is gone / not running is NOT a binary absence: it is an `Err`,
    /// so the caller can tell "image lacks the binary" from "could not probe".
    #[test]
    fn binary_present_in_container_errors_when_the_container_is_absent() {
        let tmp = tempfile::tempdir().unwrap();
        let spec = FakeSpec {
            exec_exit: 1,
            exec_stderr: "Error: No such container: pdo-sbx-r1".to_string(),
            ..FakeSpec::default()
        };
        let (docker, _log) = write_fake_docker(tmp.path(), &spec);
        let err = retry_etxtbsy(|| binary_present_in_container(&docker, "r1", 1000, 1000, "pi"))
            .unwrap_err();
        assert!(format!("{err:#}").contains("cannot answer a binary probe"));
    }

    /// A missing `docker` binary is an `Err` naming the absence (never `Ok(false)`).
    #[test]
    fn binary_present_in_container_errors_when_docker_is_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("no-such-docker");
        let err = binary_present_in_container(missing.to_str().unwrap(), "r1", 1000, 1000, "pi")
            .unwrap_err();
        assert!(format!("{err:#}").contains("not found on PATH"));
    }

    // -- Étage 2 : l'orchestration seule, docker en mémoire ----------------------

    #[test]
    fn an_absent_container_is_created_started_then_identified_in_memory() {
        let docker = ScriptedDocker::answering(|args| match args[0].as_str() {
            "container" => ScriptedDocker::fail(1, "Error: No such object: pdo-sbx-r1"),
            _ => ScriptedDocker::ok(""),
        });
        let fx = Fixtures::sample();
        super::ensure_running(&docker, "r1", &fx.spec()).unwrap();
        assert_eq!(docker.verbs(), vec!["container", "create", "start", "exec"]);
        assert_eq!(docker.calls()[1], create_args("r1", &fx.spec()));
    }

    #[test]
    fn a_transient_probe_error_never_creates_in_memory() {
        let docker = ScriptedDocker::answering(|_| {
            ScriptedDocker::fail(1, "Cannot connect to the Docker daemon")
        });
        let fx = Fixtures::sample();
        assert!(super::ensure_running(&docker, "r1", &fx.spec()).is_err());
        assert_eq!(docker.verbs(), vec!["container"]);
    }

    #[test]
    fn removing_a_gone_container_is_idempotent_in_memory() {
        let docker =
            ScriptedDocker::answering(|_| ScriptedDocker::fail(1, "Error: No such container: x"));
        assert!(super::remove(&docker, "r1").is_ok());
        assert_eq!(docker.calls(), vec![strings(&["rm", "-f", "pdo-sbx-r1"])]);
    }
}
