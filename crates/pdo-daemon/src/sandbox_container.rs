//! Cycle de vie du conteneur sandbox par Run (#406, slice C du PRD #403).
//!
//! Miroir de [`crate::sandbox_image`] / [`crate::sandbox_staging`] : pas d'`AppState`,
//! pas d'async, pas de lecture d'env dans le cœur — `&Path`/`&str`/`Vec<String>` in,
//! path-math ou `std::process::Command` out. `HOME`, l'uid/gid et le binaire courant
//! ne sont lus QUE par les résolveurs de bord.
//!
//! Ce module gère le conteneur **unique et long-vécu** d'un Run sandboxé,
//! `pdo-sbx-<run_id>` (dormant, `sleep infinity`, PID 1 = `tini` via `--init`), dans
//! lequel toutes les sessions du Run entrent par `docker exec` :
//! - [`create_args`] / [`container_name`] — construction PURE de l'argv `docker create`
//!   (identity mounts, `--user uid:gid` hôte, `--add-host`, `--init`, image, `sleep infinity`) ;
//! - [`ensure_running`] — machine à états idempotente (up → réutilise ; arrêté → `start` ;
//!   absent → `create` + `start`) dont la sonde ne confond JAMAIS une erreur transitoire
//!   (daemon Docker down, permission) avec une absence ;
//! - [`exec_prefix`] — le préfixe `docker exec -it …` qu'un nœud préposera à sa tail `claude`,
//!   avec forwarding d'env par-nœud et un marqueur de session ([`SESSION_MARKER_ENV`]) ;
//! - [`kill_session_in_container`] — kill CIBLÉ : tue le seul arbre de process porteur du
//!   marqueur (scan `/proc/*/environ`), les sessions sœurs survivent ;
//! - [`remove`] — `docker rm -f` idempotent, au `cleanup_run`.
//!
//! Les slices sœurs le CONSOMMENT mais ne sont PAS ici :
//! - #407 câble [`ensure_running`]/[`exec_prefix`]/[`kill_session_in_container`]/[`remove`]
//!   dans le run-advance et écrit **l'ADR-0030** (modèle d'exécution : réseau/auth) ;
//! - #405 fournit l'image (`ensure_image`) et le seam docker réutilisé ici
//!   ([`crate::sandbox_image::docker_bin_from_env`] / `DOCKER_NOT_FOUND_MSG`).
//!
//! ## Décisions de conception (voir la section « Sandbox » de `CONTEXT.md`)
//! - **Marqueur de session = variable d'env, pas label Docker.** [`SESSION_MARKER_ENV`] est
//!   posé par `-e` sur CHAQUE `docker exec`, donc hérité par `claude` et toute sa descendance —
//!   contrairement à un marqueur d'argv, que `exec claude` (qui écrase l'argv de `bash -c`)
//!   laisserait tomber. Le kill ciblé scanne `/proc/*/environ` pour ce marqueur.
//! - **`--user` toujours NUMÉRIQUE `uid:gid`.** `--user 1000` seul ferait résoudre le gid
//!   primaire via `/etc/passwd` (absent pour un uid arbitraire) → gid 0, bug silencieux de
//!   propriété. `-e HOME=` suffit aux écritures `~/.claude` de Claude Code.
//! - **GAP DOCUMENTÉ (uid hôte ≠ 1000).** L'image `ubuntu:24.04` livre `ubuntu`=uid/gid 1000,
//!   donc le cas laptop courant résout ENTIÈREMENT (passwd présent). Pour un uid hôte ≠ 1000 :
//!   `sudo` casse (getpwuid avant NOPASSWD) et `claude` peut casser (`os.userInfo()`). L'injection
//!   `/etc/passwd`+`/etc/group` (générer les lignes au `prepare`-time, bind-monter — pattern PDO)
//!   est **différée à une issue de suivi** (cf. `assets/sandbox/Dockerfile:33`). NE PAS éditer le
//!   Dockerfile ici : il est content-hashé (#405), toute édition périme l'image buildée.
//! - **`PDO_DAEMON_URL` réécrit au CREATE, jamais re-passé à l'exec.** Côté hôte `wrap_with_env`
//!   exporte `localhost:<port>` ; dans le conteneur `localhost` = le conteneur. Le create pose
//!   donc `-e PDO_DAEMON_URL=http://host.docker.internal:<port>` (couplé à `--add-host`), et
//!   [`exec_prefix`] ne re-passe JAMAIS cette var (un `-e PDO_DAEMON_URL` nu clobbererait la
//!   gateway par le `localhost` hôte). L'exec ne forwarde que les vars PAR-NŒUD.

#![allow(dead_code)] // Tracer bullet : consommé/câblé par #407, non câblé dans cette slice.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::sandbox_image::DOCKER_NOT_FOUND_MSG;

/// Env var portée par CHAQUE `docker exec`, héritée par `claude` et toute sa descendance ;
/// clé du kill ciblé (scan `/proc/*/environ`). N'est PAS un label Docker.
pub(crate) const SESSION_MARKER_ENV: &str = "PDO_SBX_SESSION";

// -- type valeur (assemblé par les résolveurs, nourrit les builders purs) ----

/// Tout ce dont [`create_args`] a besoin pour construire l'argv `docker create` d'un conteneur
/// sandbox. Assemblé par le caller (#407) à partir des résolveurs de bord + des sorties de
/// #404/#405 ; les builders purs ne lisent jamais l'environnement.
pub(crate) struct ContainerSpec<'a> {
    /// `pdo-sandbox:h-<hash>` ([`crate::sandbox_image::local_image_ref`]).
    pub image_ref: &'a str,
    /// Repo cible, monté rw à son chemin absolu hôte — couvre repo + tous les worktrees de
    /// nœuds (sous `.pdo/runs/`) + `.pdo/prompts` en UN seul mount.
    pub repo_root: &'a Path,
    /// `-w` au create : cwd par défaut du conteneur (cosmétique — le conteneur ne fait que
    /// `sleep` ; le cwd load-bearing est par-exec, cf. [`exec_prefix`]).
    pub run_worktree: &'a Path,
    /// *Staged Claude home* (#404) → `<host_home>/.claude:rw`.
    pub staged_home: &'a Path,
    /// `.claude.json` sibling (#404) → `<host_home>/.claude.json:rw`.
    pub staged_json: &'a Path,
    /// Binaire `pdo` hôte → `/usr/local/bin/pdo:ro`.
    pub pdo_bin: &'a Path,
    /// `$HOME` hôte : `-e HOME=` + racine des cibles de mount `.claude`/`.claude.json`.
    pub host_home: &'a Path,
    /// uid hôte (`--user <uid>:<gid>`, numérique).
    pub uid: u32,
    /// gid hôte (`--user <uid>:<gid>`, numérique).
    pub gid: u32,
    /// Port du daemon hôte : `-e PDO_DAEMON_URL=http://host.docker.internal:<port>`.
    pub daemon_port: u16,
}

// -- builders purs (golden-testés) -------------------------------------------

/// Nom déterministe du conteneur d'un Run : `pdo-sbx-<run_id>`. Par-Run → kill/destruction
/// CIBLÉS (jamais un balayage global).
pub(crate) fn container_name(run_id: &str) -> String {
    format!("pdo-sbx-{run_id}")
}

/// Argv `docker create` (après le binaire `docker`). Ordre canonique FIGÉ par le golden test :
/// `--init`, `--name`, `--user` numérique, `--add-host`, `-w`, les 4 `-e`, les 4 `-v`, l'image,
/// puis le trailing `sleep infinity` (le conteneur dort ; les tails entrent par `docker exec`).
pub(crate) fn create_args(run_id: &str, spec: &ContainerSpec) -> Vec<String> {
    vec![
        "create".to_string(),
        // --init : tini comme PID 1, reape les enfants reparentés après un kill ciblé (sinon
        // zombies permanents, `sleep infinity` ne reape pas). Docker embarque tini >= 1.13.
        "--init".to_string(),
        "--name".to_string(),
        container_name(run_id),
        // --user NUMÉRIQUE (uid:gid) : `--user <uid>` seul résoudrait le gid via /etc/passwd
        // (absent pour un uid arbitraire) → gid 0.
        "--user".to_string(),
        format!("{}:{}", spec.uid, spec.gid),
        // --add-host : la gateway que `-e PDO_DAEMON_URL` ci-dessous pointe.
        "--add-host".to_string(),
        "host.docker.internal:host-gateway".to_string(),
        // -w au create (cosmétique, satisfait l'AC ; load-bearing = par-exec).
        "-w".to_string(),
        spec.run_worktree.display().to_string(),
        // Vars RUN-CONSTANTES, posées une fois au create.
        "-e".to_string(),
        format!("HOME={}", spec.host_home.display()),
        "-e".to_string(),
        format!(
            "PDO_DAEMON_URL=http://host.docker.internal:{}",
            spec.daemon_port
        ),
        "-e".to_string(),
        format!("PDO_RUN_ID={run_id}"),
        "-e".to_string(),
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".to_string(),
        // Identity mounts. UN mount repo couvre repo + worktrees + prompts au MÊME chemin absolu
        // (invariant load-bearing D3 : le dirname encodé de merge_back doit matcher).
        "-v".to_string(),
        format!("{r}:{r}:rw", r = spec.repo_root.display()),
        "-v".to_string(),
        format!(
            "{}:{}:rw",
            spec.staged_home.display(),
            spec.host_home.join(".claude").display()
        ),
        "-v".to_string(),
        format!(
            "{}:{}:rw",
            spec.staged_json.display(),
            spec.host_home.join(".claude.json").display()
        ),
        "-v".to_string(),
        format!("{}:/usr/local/bin/pdo:ro", spec.pdo_bin.display()),
        // Image, puis la commande dormante.
        spec.image_ref.to_string(),
        "sleep".to_string(),
        "infinity".to_string(),
    ]
}

/// Préfixe `docker exec` d'une session de nœud (après le binaire `docker` ; #407 y ajoute la
/// tail `claude`). Ordre canonique FIGÉ par le golden test.
///
/// - `-e PDO_SBX_SESSION=<marker>` : le marqueur hérité par `claude` et sa descendance (clé du
///   kill ciblé) ;
/// - `-e PDO_NODE_ID` / `-e PDO_NODE_ITER` NUS : forwardent la valeur du shell hôte que
///   `wrap_with_env` a exportée (vars PAR-NŒUD qui varient dans le Run) ;
/// - `-w <workdir>` : le worktree DU NŒUD (valeur load-bearing D3, ≠ le `-w` cosmétique du create).
///
/// **JAMAIS** `-e PDO_DAEMON_URL` ici (posé au create vers `host.docker.internal` ; un `-e`
/// nu re-forwarderait le `localhost` hôte et clobbererait la gateway).
pub(crate) fn exec_prefix(
    run_id: &str,
    uid: u32,
    gid: u32,
    workdir: &Path,
    marker: &str,
) -> Vec<String> {
    // Delegate with an empty catalogue: the output stays byte-identical to the
    // #406 golden (the base three `-e` only, no per-node values).
    exec_prefix_with_env(run_id, uid, gid, workdir, marker, &[])
}

/// Like [`exec_prefix`] but forwards a **dynamic per-node env catalogue** into the
/// container as explicit `-e KEY=VALUE` (#407, D6). A `script` node's I/O arrives
/// as env vars (`PDO_ARTIFACTS_DIR`, `PDO_INPUT_<port>`, `PDO_OUTPUT_<port>`,
/// `PDO_VAR_<name>`), whose values are known at spawn time but are **not** among
/// the statically-forwarded `-e PDO_NODE_ID`/`-e PDO_NODE_ITER` — a bare `-e KEY`
/// only forwards a host-shell value, and the host shell exports none of the
/// catalogue in sandbox mode (D6). So they must be passed as valued `-e KEY=VALUE`
/// inserted **before** the container name.
///
/// **Never `PDO_DAEMON_URL`** (a valued or bare `-e` would clobber the
/// host-gateway URL posted at `create`): it is filtered here as a belt-and-braces
/// invariant, even though the catalogue should never contain it. Order is FIGÉ by
/// the golden test: the base three `-e` first, then each catalogue `-e K=V` in
/// order, then `--user`, `-w`, container.
pub(crate) fn exec_prefix_with_env(
    run_id: &str,
    uid: u32,
    gid: u32,
    workdir: &Path,
    marker: &str,
    extra_env: &[(String, String)],
) -> Vec<String> {
    let mut args = vec![
        "exec".to_string(),
        "-i".to_string(),
        "-t".to_string(),
        "-e".to_string(),
        format!("{SESSION_MARKER_ENV}={marker}"),
        "-e".to_string(),
        "PDO_NODE_ID".to_string(),
        "-e".to_string(),
        "PDO_NODE_ITER".to_string(),
    ];
    for (k, v) in extra_env {
        // Invariant: PDO_DAEMON_URL is posed at create → host.docker.internal; a
        // re-passed `-e` (bare or valued) would clobber the gateway.
        if k == "PDO_DAEMON_URL" {
            continue;
        }
        args.push("-e".to_string());
        args.push(format!("{k}={v}"));
    }
    args.push("--user".to_string());
    args.push(format!("{uid}:{gid}"));
    args.push("-w".to_string());
    args.push(workdir.display().to_string());
    args.push(container_name(run_id));
    args
}

// -- effets docker (sync std::process::Command, anyhow + .context) -----------

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
fn probe_state(docker_bin: &str, name: &str) -> Result<ContainerState> {
    let output = match Command::new(docker_bin)
        .args(["container", "inspect", "-f", "{{.State.Running}}", name])
        .output()
    {
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

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        match stdout.trim() {
            "true" => Ok(ContainerState::Running),
            "false" => Ok(ContainerState::Stopped),
            other => anyhow::bail!(
                "unexpected `docker container inspect` output while probing `{name}`: {other:?} \
                 (expected `true`/`false`) — refusing to treat as an absent container"
            ),
        }
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if is_absent_stderr(&stderr) {
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

/// Provisionneur IDEMPOTENT du conteneur (miroir de [`crate::sandbox_image::ensure_image`]) :
/// sonde → up → no-op ; arrêté → `start` ; absent → `create` + `start`. Toute erreur de sonde
/// (transitoire, docker absent) remonte via `?` sans jamais être traitée comme une absence.
pub(crate) fn ensure_running(docker_bin: &str, run_id: &str, spec: &ContainerSpec) -> Result<()> {
    let name = container_name(run_id);
    match probe_state(docker_bin, &name)? {
        ContainerState::Running => Ok(()),
        ContainerState::Stopped => start_container(docker_bin, &name),
        ContainerState::Absent => {
            create_container(docker_bin, run_id, spec)?;
            start_container(docker_bin, &name)
        }
    }
}

/// `docker` + [`create_args`]. `create` et `start` sont DEUX primitives (pas `run -d`) : la
/// branche Stopped a de toute façon besoin de `start`, et un conteneur arrêté ne se `run` pas à
/// nouveau. Pas de `--restart` (PDO possède le cycle de vie ; `unless-stopped` ressusciterait des
/// conteneurs que PDO croit finis → fuite). Course au create (v1, mono-appelant par `run_id`) :
/// un stderr `Conflict … already in use` est mappé sur un **succès bénin** (le `start` suivant est
/// idempotent).
fn create_container(docker_bin: &str, run_id: &str, spec: &ContainerSpec) -> Result<()> {
    let args = create_args(run_id, spec);
    let output = match Command::new(docker_bin).args(&args).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker create` for the sandbox container");
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_name_conflict_stderr(&stderr) {
        return Ok(()); // course bénigne : le conteneur existe déjà, on enchaîne `start`.
    }
    anyhow::bail!(
        "failed to create the sandbox container `{}` — `docker create` exited with {}: {stderr}",
        container_name(run_id),
        output.status
    )
}

/// `docker start <name>` (`docker start` ne prend pas `-d`). Non-zéro → bail avec le stderr docker.
fn start_container(docker_bin: &str, name: &str) -> Result<()> {
    let output = match Command::new(docker_bin).args(["start", name]).output() {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker start` for the sandbox container");
        }
    };
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "failed to start the sandbox container `{name}` — `docker start` exited with {}: {stderr}",
            output.status
        )
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
pub(crate) fn kill_session_in_container(
    docker_bin: &str,
    run_id: &str,
    marker: &str,
    uid: u32,
    gid: u32,
) -> Result<()> {
    let name = container_name(run_id);
    let user = format!("{uid}:{gid}");
    let script = kill_one_liner(marker);
    let output = match Command::new(docker_bin)
        .arg("exec")
        .arg("--user")
        .arg(&user)
        .arg(&name)
        .arg("sh")
        .arg("-c")
        .arg(&script)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker exec` to kill a sandbox session");
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_absent_stderr(&stderr) {
        return Ok(()); // best-effort : plus de conteneur, plus rien à tuer.
    }
    anyhow::bail!(
        "failed to kill session `{marker}` in sandbox container `{name}` — \
         `docker exec` exited with {}: {stderr}",
        output.status
    )
}

/// `docker rm -f <name>` au `cleanup_run`. Idempotent : conteneur déjà absent (sentinelle) →
/// `Ok`. C'est LE verbe « destroy / destruction » du conteneur (distinct de
/// [`kill_session_in_container`], qui ne tue qu'un arbre de session).
pub(crate) fn remove(docker_bin: &str, run_id: &str) -> Result<()> {
    let name = container_name(run_id);
    let output = match Command::new(docker_bin)
        .arg("rm")
        .arg("-f")
        .arg(&name)
        .output()
    {
        Ok(output) => output,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(anyhow::Error::new(e)).context(DOCKER_NOT_FOUND_MSG);
        }
        Err(e) => {
            return Err(e).context("failed to run `docker rm -f` for the sandbox container");
        }
    };
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    if is_absent_stderr(&stderr) {
        return Ok(()); // idempotent : déjà supprimé.
    }
    anyhow::bail!(
        "failed to remove the sandbox container `{name}` — `docker rm -f` exited with {}: {stderr}",
        output.status
    )
}

// -- helpers (privés) --------------------------------------------------------

/// Sentinelle docker « conteneur absent », insensible à la casse. Couvre les deux libellés
/// (`No such container` de `rm`/`exec`, `No such object` d'`inspect`). Une comparaison de
/// substring, pas d'exit-code, car un exit != 0 peut aussi être transitoire.
fn is_absent_stderr(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("no such container") || s.contains("no such object")
}

/// Sentinelle docker « nom déjà pris » (course au create), insensible à la casse.
fn is_name_conflict_stderr(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("conflict") || s.contains("already in use")
}

/// One-liner `sh -c` du kill ciblé (marqueur single-quoté). `grep -qazx` : `-z` enregistrements
/// NUL (`/environ` est NUL-séparé), `-x` match ligne ENTIÈRE (pas de faux positif sur un préfixe
/// du marqueur d'une session sœur), `-a` force le mode texte. `TERM` puis, après 2 s, `KILL`.
fn kill_one_liner(marker: &str) -> String {
    let target = sh_single_quote(&format!("{SESSION_MARKER_ENV}={marker}"));
    format!(
        "t={target}; \
k() {{ for e in /proc/[0-9]*/environ; do p=${{e%/environ}}; p=${{p#/proc/}}; \
grep -qazx \"$t\" \"$e\" 2>/dev/null && kill -\"$1\" \"$p\" 2>/dev/null; done; }}; \
k TERM; sleep 2; k KILL"
    )
}

/// Single-quote une chaîne pour un `sh -c` (miroir de `tmux_session_manager::sh_single_quote`).
fn sh_single_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for ch in s.chars() {
        if ch == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(ch);
        }
    }
    out.push('\'');
    out
}

// -- résolveurs de bord (seuls lecteurs env/uid ; les tests injectent directement) -------

/// `$HOME` hôte. `None` si absent. Câblé par le daemon (#407) ; les unit tests injectent des
/// temp dirs et bypassent ce résolveur.
pub(crate) fn host_home_from_env() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// uid hôte via `getuid(2)` (syscall, pas d'env → pas de suffixe `_from_env`).
pub(crate) fn host_uid() -> u32 {
    // SAFETY: `getuid` a la signature `() -> uid_t`, ne prend aucun argument, ne déréférence
    // aucun pointeur et réussit toujours (POSIX : « The getuid() function shall always be
    // successful »). Aucune précondition à satisfaire.
    unsafe { libc::getuid() }
}

/// gid hôte via `getgid(2)`.
pub(crate) fn host_gid() -> u32 {
    // SAFETY: idem [`host_uid`] — `getgid` ne prend aucun argument et réussit toujours.
    unsafe { libc::getgid() }
}

/// Chemin du binaire `pdo` courant à bind-monter dans le conteneur (`/usr/local/bin/pdo:ro`).
/// Miroir de l'usage `std::env::current_exe` de `lib.rs`.
pub(crate) fn pdo_bin_path() -> Result<PathBuf> {
    std::env::current_exe()
        .context("failed to determine the pdo binary path to bind-mount into the sandbox container")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

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

    // -- 1. create_args golden (AC#1) ----------------------------------------

    #[test]
    fn create_args_golden() {
        let fx = Fixtures::sample();
        let args = create_args("r1", &fx.spec());
        let expected = strings(&[
            "create",
            "--init",
            "--name",
            "pdo-sbx-r1",
            "--user",
            "1000:1000",
            "--add-host",
            "host.docker.internal:host-gateway",
            "-w",
            "/repo/.pdo/runs/r1/worktree",
            "-e",
            "HOME=/home/u",
            "-e",
            "PDO_DAEMON_URL=http://host.docker.internal:6172",
            "-e",
            "PDO_RUN_ID=r1",
            "-e",
            "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1",
            "-v",
            "/repo:/repo:rw",
            "-v",
            "/sb/r1/claude-home:/home/u/.claude:rw",
            "-v",
            "/sb/r1/.claude.json:/home/u/.claude.json:rw",
            "-v",
            "/host/bin/pdo:/usr/local/bin/pdo:ro",
            "pdo-sandbox:h-abc123",
            "sleep",
            "infinity",
        ]);
        assert_eq!(args, expected);
    }

    // -- 2. exec_prefix golden (AC#1) ----------------------------------------

    #[test]
    fn exec_prefix_golden() {
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/n1/iter-1");
        let args = exec_prefix("r1", 1000, 1000, wt, "pdo-r1-n1-iter-1");
        let expected = strings(&[
            "exec",
            "-i",
            "-t",
            "-e",
            "PDO_SBX_SESSION=pdo-r1-n1-iter-1",
            "-e",
            "PDO_NODE_ID",
            "-e",
            "PDO_NODE_ITER",
            "--user",
            "1000:1000",
            "-w",
            "/repo/.pdo/runs/r1/nodes/n1/iter-1",
            "pdo-sbx-r1",
        ]);
        assert_eq!(args, expected);
        // D4 : jamais PDO_DAEMON_URL sur l'exec (clobbererait la gateway du create).
        assert!(
            !args.iter().any(|a| a.contains("PDO_DAEMON_URL")),
            "exec_prefix ne doit jamais re-passer PDO_DAEMON_URL"
        );
    }

    // -- 2b. exec_prefix_with_env golden : catalogue script (#407, D6) --------

    #[test]
    fn exec_prefix_with_env_golden() {
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/n1/iter-1");
        // A realistic `script` node catalogue (order preserved), plus a
        // PDO_DAEMON_URL that MUST be filtered out (invariant).
        let env = vec![
            (
                "PDO_ARTIFACTS_DIR".to_string(),
                "/repo/.pdo/runs/r1/worktree/.pdo/artifacts".to_string(),
            ),
            (
                "PDO_OUTPUT_out".to_string(),
                "/repo/.pdo/runs/r1/worktree/.pdo/artifacts/n1/iter-1/out/output.md".to_string(),
            ),
            ("PDO_VAR_x".to_string(), "hello".to_string()),
            // Adversarial: must be dropped, never forwarded.
            (
                "PDO_DAEMON_URL".to_string(),
                "http://localhost:6172".to_string(),
            ),
        ];
        let args = exec_prefix_with_env("r1", 1000, 1000, wt, "pdo-r1-n1-iter-1", &env);
        let expected = strings(&[
            "exec",
            "-i",
            "-t",
            "-e",
            "PDO_SBX_SESSION=pdo-r1-n1-iter-1",
            "-e",
            "PDO_NODE_ID",
            "-e",
            "PDO_NODE_ITER",
            "-e",
            "PDO_ARTIFACTS_DIR=/repo/.pdo/runs/r1/worktree/.pdo/artifacts",
            "-e",
            "PDO_OUTPUT_out=/repo/.pdo/runs/r1/worktree/.pdo/artifacts/n1/iter-1/out/output.md",
            "-e",
            "PDO_VAR_x=hello",
            "--user",
            "1000:1000",
            "-w",
            "/repo/.pdo/runs/r1/nodes/n1/iter-1",
            "pdo-sbx-r1",
        ]);
        assert_eq!(args, expected, "catalogue `-e K=V` inséré avant le conteneur");
        // Invariant : PDO_DAEMON_URL jamais re-passé, même présent dans le catalogue.
        assert!(
            !args.iter().any(|a| a.contains("PDO_DAEMON_URL")),
            "PDO_DAEMON_URL doit être filtré du catalogue"
        );
    }

    #[test]
    fn exec_prefix_empty_env_equals_bare_prefix() {
        // exec_prefix (délégation `&[]`) == exec_prefix_with_env(&[]) : garde de
        // non-régression du golden #406.
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/n1/iter-1");
        assert_eq!(
            exec_prefix("r1", 1000, 1000, wt, "m"),
            exec_prefix_with_env("r1", 1000, 1000, wt, "m", &[]),
        );
    }

    // -- 3-8. ensure_running / probe (AC#2) ----------------------------------

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
        // Le create se termine par la commande dormante.
        assert!(
            lines.contains(&"sleep".to_string()) && lines.contains(&"infinity".to_string()),
            "create doit poser `sleep infinity`"
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
        // Aucune invocation docker n'a pu tourner → pas de log.
        assert!(
            !tmp.path().join("argv.log").exists(),
            "docker absent → argv-log vide"
        );
    }

    // -- 9. kill ciblé (AC#3) ------------------------------------------------

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

    // -- 10-12. remove (AC#4 / AC#5) -----------------------------------------

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

        // Idempotent : `rm -f` d'un conteneur absent → Ok.
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

    // -- 13. schéma de nom ---------------------------------------------------

    #[test]
    fn container_name_schema() {
        assert_eq!(container_name("r"), "pdo-sbx-r");
        assert_eq!(container_name("20260101-abcdef"), "pdo-sbx-20260101-abcdef");
    }
}
