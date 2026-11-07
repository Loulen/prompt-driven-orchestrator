//! Cycle de vie du conteneur sandbox par Run (#406, slice C du PRD #403).
//!
//! `HOME`, l'uid/gid et le binaire courant ne sont lus QUE par les résolveurs de bord
//! ([`crate::adapters::host`]).
//!
//! Ce module gère le conteneur **unique et long-vécu** d'un Run sandboxé,
//! `pdo-sbx-<run_id>` (dormant, `sleep infinity`, PID 1 = `tini` via `--init`), dans
//! lequel toutes les sessions du Run entrent par `docker exec`.
//!
//! ## Décisions de conception (voir la section « Sandbox » de `CONTEXT.md`)
//! - **Marqueur de session = variable d'env, pas label Docker.** [`SESSION_MARKER_ENV`] est
//!   posé par `-e` sur CHAQUE `docker exec`, donc hérité par `claude` et toute sa descendance —
//!   contrairement à un marqueur d'argv, que `exec claude` (qui écrase l'argv de `bash -c`)
//!   laisserait tomber. Le kill ciblé scanne `/proc/*/environ` pour ce marqueur.
//! - **`--user` toujours NUMÉRIQUE `uid:gid`.** `--user 1000` seul ferait résoudre le gid
//!   primaire via `/etc/passwd` (absent pour un uid arbitraire) → gid 0, bug silencieux de
//!   propriété. `-e HOME=` suffit aux écritures `~/.claude` de Claude Code.
//! - **L'identité de l'uid hôte est posée sur le conteneur, pas dans l'image (#414).** L'image
//!   `ubuntu:24.04` ne connaît que l'uid 1000 (`ubuntu`) ; pour tout autre uid hôte, `sudo`
//!   appelle `getpwuid()` avant NOPASSWD et abandonne (« you do not exist in the passwd
//!   database »), et `whoami` échoue. [`crate::application::container::ensure_running`] lance donc, juste après le `start`, un
//!   `docker exec --user 0:0` qui **ajoute** les lignes manquantes aux `/etc/passwd` et
//!   `/etc/group` RÉELS de l'image, derrière une garde `getent` ([`identity_script`]).
//!   **Pas un bind-mount** (le mécanisme initialement prescrit) : mesuré, un `/etc/passwd`
//!   bind-monté casse `useradd`/`apt` en `:ro` **comme** en `:rw` (`rename()` par-dessus un
//!   mount de fichier), et exigerait de dupliquer la baseline d'une image que l'utilisateur peut
//!   éditer. NE PAS éditer le Dockerfile : il est content-hashé (#405), toute édition périme
//!   l'image buildée — et son commentaire ligne 33 décrit désormais le comportement livré.
//! - **L'env par profil de staging est posé au CREATE (#468).** Ce sont des constantes de Run,
//!   pas des variables de nœud : le chemin `docker exec` a sa propre liste par-nœud
//!   ([`exec_prefix_with_env`]) et n'est pas concerné. Ce module possède
//!   [`RUN_CONSTANT_ENV_KEYS`] — la liste des clés qu'un profil ne peut pas redéfinir — parce
//!   qu'il possède les `-e` qui les posent ; `sandbox_profile` la CONSOMME pour rejeter le PUT
//!   (même discipline anti-dérive que [`daemon_url`], cf. #447).
//! - **`PDO_DAEMON_URL` réécrit au CREATE, jamais re-passé à l'exec.** Côté hôte `wrap_with_env`
//!   exporte `localhost:<port>` ; dans le conteneur `localhost` = le conteneur. Le create pose
//!   donc `-e PDO_DAEMON_URL=http://host.docker.internal:<port>` (couplé à `--add-host`), et
//!   [`exec_prefix`] ne re-passe JAMAIS cette var (un `-e PDO_DAEMON_URL` nu clobbererait la
//!   gateway par le `localhost` hôte). L'exec ne forwarde que les vars PAR-NŒUD.
//! - **[`daemon_url`] est le résolveur UNIQUE de cette URL (#447).** Ce module possède le
//!   hostname de la gateway parce qu'il possède le `--add-host` qui la crée : le littéral
//!   n'apparaît donc qu'ici. Le `-e PDO_DAEMON_URL` du create **et** le texte des préambules
//!   (manager, ADR-0030 §8) passent par le même appel — c'est ce qui interdit qu'un chemin
//!   résolve pendant que l'autre reste sur `localhost`.

//!
//! Ce module est la moitié **pure** : les noms, les argv (`create`, `exec`, kill ciblé,
//! identité) et les sentinelles de stderr. Les invocations passent par le port
//! [`crate::application::ports::DockerCli`], dans [`crate::application::container`].

pub mod create;

use std::path::Path;

pub use create::{create_args, identity_exec_args, identity_script, ContainerSpec};

/// Env var portée par CHAQUE `docker exec`, héritée par `claude` et toute sa descendance ;
/// clé du kill ciblé (scan `/proc/*/environ`). N'est PAS un label Docker.
pub const SESSION_MARKER_ENV: &str = "PDO_SBX_SESSION";

/// Knob Claude Code posé au create, **délibérément absent** de
/// [`RUN_CONSTANT_ENV_KEYS`] : un profil qui veut ré-autoriser le trafic non essentiel a
/// une raison légitime de le faire, et la fusion de [`create_args`] remplace alors la
/// valeur **en place** (un seul `-e`, à la même position) au lieu d'en ajouter un second.
const CLAUDE_TRAFFIC_ENV: &str = "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC";

/// Nom d'utilisateur **et** de groupe synthétisés dans le conteneur pour l'uid/gid hôte
/// ([`crate::application::container::ensure_running`]). Constante, jamais `$USER` : aucun chemin load-bearing ne lit ce
/// nom (le `claude` de l'image est un binaire **Bun**, qui lit `$HOME`/`$USER` et ignore la
/// base passwd ; `git` échoue sur le domaine `(none)`, pas sur le nom), donc lire
/// l'environnement ajouterait le premier résolveur de nom d'utilisateur du dépôt pour un
/// gain nul. `ubuntu:24.04` ne livre aucun `pdo` — pas de collision de nom.
const IDENTITY_NAME: &str = "pdo";

/// Les trois vars run-constantes qu'un profil de staging ne peut **pas** redéfinir (#468).
///
/// Possédées ici parce que c'est [`create_args`] qui les pose : le littéral n'existe donc
/// qu'à un seul endroit, et `sandbox_profile::validate_env_key` s'y adosse pour rendre un
/// **400 nommant la clé** au PUT (jamais un skip silencieux). Le filtre de [`create_args`]
/// est la ceinture-bretelles du même invariant, pas le garde-fou principal — exactement le
/// rôle que joue déjà le saut de `PDO_DAEMON_URL` dans [`exec_prefix_with_env`].
///
/// Pourquoi ces trois-là, et pas les quatre `-e` du create :
/// - `HOME` : les mounts `.claude` / `.claude.json` sont calculés depuis lui. Un `HOME`
///   écrasé casserait les deux d'un coup — le conteneur chercherait sa config là où rien
///   n'est monté, et l'agent démarrerait sans credentials ni CLAUDE.md.
/// - `PDO_DAEMON_URL` : posé vers la gateway `host.docker.internal` ; une autre valeur
///   coupe le plan de contrôle (`pdo complete` depuis le conteneur).
/// - `PDO_RUN_ID` : identité du Run, lue par la CLI `pdo` montée dans le conteneur.
///
/// La relation « toute clé réservée est bien posée par le create » est prouvée par un test
/// dans ce module. Elle est **unidirectionnelle** : [`CLAUDE_TRAFFIC_ENV`] est posé sans
/// être réservé, et c'est voulu.
pub const RUN_CONSTANT_ENV_KEYS: &[&str] = &["HOME", "PDO_DAEMON_URL", "PDO_RUN_ID"];

/// Hostname du daemon **vu depuis le côté hôte** : le conteneur exclu, `localhost`
/// désigne bien la machine qui écoute.
const HOST_SIDE_HOST: &str = "localhost";

/// Hostname de la gateway hôte **vu depuis l'intérieur du conteneur**, matérialisée
/// par le `--add-host host.docker.internal:host-gateway` de [`create_args`]. Dans le
/// conteneur, `localhost` désigne le conteneur : c'est ce hostname-là, et lui seul,
/// qui joint le daemon.
const CONTAINER_SIDE_HOST: &str = "host.docker.internal";

/// Résolveur PUR de l'URL du daemon **telle qu'elle est joignable depuis le côté où
/// l'agent s'exécute** (#447).
///
/// Le bug qu'il ferme : l'URL existait sous DEUX représentations du même fait —
/// l'env `PDO_DAEMON_URL` (résolue, donc correcte dans le conteneur) et le **texte**
/// du préambule (codé en dur sur `localhost`). Un agent sandboxé qui obéissait aux
/// `curl` de son propre préambule tapait dans le vide et en concluait, de bonne foi,
/// que le daemon était mort — panne silencieuse, intermittente et *affirmative*.
/// Un seul résolveur, consommé par les deux, rend la dérive impossible (leçon #373).
///
/// **Ce résolveur ne s'applique PAS aux exports d'env côté hôte du wrapper
/// `docker exec`** (`pdo_session::domain::launch::wrap_with_env`) : ceux-là tournent AVANT
/// le `docker exec`, ne traversent pas dans le conteneur, et doivent donc rester
/// `localhost` (ADR-0030 §5). Ils appellent avec `sandboxed = false` — l'argument
/// nomme le côté d'exécution, pas le mode du Run.
pub fn daemon_url(port: u16, sandboxed: bool) -> String {
    let host = if sandboxed {
        CONTAINER_SIDE_HOST
    } else {
        HOST_SIDE_HOST
    };
    format!("http://{host}:{port}")
}

/// Nom déterministe du conteneur d'un Run : `pdo-sbx-<run_id>`. Par-Run → kill/destruction
/// CIBLÉS (jamais un balayage global).
pub fn container_name(run_id: &str) -> String {
    format!("pdo-sbx-{run_id}")
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
pub fn exec_prefix(run_id: &str, uid: u32, gid: u32, workdir: &Path, marker: &str) -> Vec<String> {
    exec_prefix_with_env(run_id, uid, gid, workdir, marker, &[])
}

/// Like [`exec_prefix`] but forwards a **dynamic per-node env catalogue** into the
/// container (#407, D6): a `script` node's I/O (`PDO_ARTIFACTS_DIR`,
/// `PDO_INPUT_<port>`, `PDO_OUTPUT_<port>`, `PDO_VAR_<name>`), the extensions'
/// variables, a staging set's env. Each is a **bare** `-e KEY`, like
/// `PDO_NODE_ID`: `docker` reads the value from the host shell, which exported
/// it just before, so no value (a token, a key) ever sits in the argv of the
/// `docker` client, where any local user reads it with `ps`.
///
/// **Never `PDO_DAEMON_URL`** (a bare `-e` would clobber the host-gateway URL
/// posted at `create` with the host's `localhost`): it is filtered here as a
/// belt-and-braces invariant, even though the catalogue should never contain it.
/// Order is FIGÉ by the golden test: the base three `-e` first, then each
/// catalogue `-e K` in order, then `--user`, `-w`, container.
pub fn exec_prefix_with_env(
    run_id: &str,
    uid: u32,
    gid: u32,
    workdir: &Path,
    marker: &str,
    forwarded: &[String],
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
    for name in forwarded {
        // Don't re-pass PDO_DAEMON_URL: posed at create → host.docker.internal, a
        // second `-e` clobbers the gateway.
        if name == "PDO_DAEMON_URL" {
            continue;
        }
        args.push("-e".to_string());
        args.push(name.clone());
    }
    args.push("--user".to_string());
    args.push(format!("{uid}:{gid}"));
    args.push("-w".to_string());
    args.push(workdir.display().to_string());
    args.push(container_name(run_id));
    args
}

/// Sentinelle docker « conteneur absent », insensible à la casse. Couvre les deux libellés
/// (`No such container` de `rm`/`exec`, `No such object` d'`inspect`). Une comparaison de
/// substring, pas d'exit-code, car un exit != 0 peut aussi être transitoire.
pub fn is_absent_stderr(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("no such container") || s.contains("no such object")
}

/// Sentinelle docker « nom déjà pris » (course au create), insensible à la casse.
pub fn is_name_conflict_stderr(stderr: &str) -> bool {
    let s = stderr.to_lowercase();
    s.contains("conflict") || s.contains("already in use")
}

/// One-liner `sh -c` du kill ciblé (marqueur single-quoté). `grep -qazx` : `-z` enregistrements
/// NUL (`/environ` est NUL-séparé), `-x` match ligne ENTIÈRE (pas de faux positif sur un préfixe
/// du marqueur d'une session sœur), `-a` force le mode texte. `TERM` puis, après 2 s, `KILL`.
pub fn kill_one_liner(marker: &str) -> String {
    let target = sh_single_quote(&format!("{SESSION_MARKER_ENV}={marker}"));
    format!(
        "t={target}; \
k() {{ for e in /proc/[0-9]*/environ; do p=${{e%/environ}}; p=${{p#/proc/}}; \
grep -qazx \"$t\" \"$e\" 2>/dev/null && kill -\"$1\" \"$p\" 2>/dev/null; done; }}; \
k TERM; sleep 2; k KILL"
    )
}

/// Single-quote une chaîne pour un `sh -c` (miroir de `pdo_session::domain::launch::sh_single_quote`).
pub fn sh_single_quote(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Les deux côtés d'exécution rendent des hostnames DIFFÉRENTS, et le port suit.
    /// C'est la propriété que le bug violait : une seule des deux formes existait.
    #[test]
    fn daemon_url_resolves_per_execution_side() {
        assert_eq!(daemon_url(6193, false), "http://localhost:6193");
        assert_eq!(
            daemon_url(6193, true),
            "http://host.docker.internal:6193",
            "un agent dans le conteneur ne joint le daemon que par la gateway"
        );
        assert_ne!(daemon_url(6193, true), daemon_url(6193, false));
    }

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

    #[test]
    fn exec_prefix_with_env_golden() {
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/n1/iter-1");
        let env = strings(&[
            "PDO_ARTIFACTS_DIR",
            "PDO_OUTPUT_out",
            "PDO_VAR_x",
            // Adversarial: must be dropped, never forwarded.
            "PDO_DAEMON_URL",
        ]);
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
            "PDO_ARTIFACTS_DIR",
            "-e",
            "PDO_OUTPUT_out",
            "-e",
            "PDO_VAR_x",
            "--user",
            "1000:1000",
            "-w",
            "/repo/.pdo/runs/r1/nodes/n1/iter-1",
            "pdo-sbx-r1",
        ]);
        assert_eq!(
            args, expected,
            "catalogue `-e K` (by name) inséré avant le conteneur"
        );
        // Invariant : PDO_DAEMON_URL jamais re-passé, même présent dans le catalogue.
        assert!(
            !args.iter().any(|a| a.contains("PDO_DAEMON_URL")),
            "PDO_DAEMON_URL doit être filtré du catalogue"
        );
    }

    #[test]
    fn exec_prefix_empty_env_equals_bare_prefix() {
        let wt = Path::new("/repo/.pdo/runs/r1/nodes/n1/iter-1");
        assert_eq!(
            exec_prefix("r1", 1000, 1000, wt, "m"),
            exec_prefix_with_env("r1", 1000, 1000, wt, "m", &[]),
        );
    }

    #[test]
    fn container_name_schema() {
        assert_eq!(container_name("r"), "pdo-sbx-r");
        assert_eq!(container_name("20260101-abcdef"), "pdo-sbx-20260101-abcdef");
    }
}
