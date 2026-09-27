//! L'argv `docker create` du conteneur d'un Run, et l'injection d'identité qui suit son
//! démarrage (#414). Pur : aucun appel docker ici.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::{
    container_name, daemon_url, sh_single_quote, CLAUDE_TRAFFIC_ENV, CONTAINER_SIDE_HOST,
    IDENTITY_NAME, RUN_CONSTANT_ENV_KEYS,
};
use crate::domain::staging::StagedMount;

/// Tout ce dont [`create_args`] a besoin pour construire l'argv `docker create` d'un conteneur
/// sandbox. Assemblé par le caller (#407) à partir des résolveurs de bord + des sorties de
/// #404/#405 ; les builders purs ne lisent jamais l'environnement.
pub struct ContainerSpec<'a> {
    /// `pdo-sandbox:h-<hash>` ([`crate::domain::image::local_image_ref`]).
    pub image_ref: &'a str,
    /// Repo cible, monté rw à son chemin absolu hôte — couvre repo + tous les worktrees de
    /// nœuds (sous `.pdo/runs/`) + `.pdo/prompts` en UN seul mount.
    pub repo_root: &'a Path,
    /// `-w` au create : cwd par défaut du conteneur (cosmétique — le conteneur ne fait que
    /// `sleep` ; le cwd load-bearing est par-exec, cf. [`super::exec_prefix`]).
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
    /// Mounts d'**exception `$HOME`** du profil de staging (#432, ADR-0031 §4) —
    /// calculés par [`crate::adapters::staging::extra_mounts`] depuis la liste d'entrées
    /// GELÉE croisée avec le disque. Queue **variable** : vide pour `minimal`/`full`
    /// non édités, d'où un argv byte-identique à #406 (propriété que le golden test
    /// pin explicitement).
    ///
    /// Note pour [`crate::application::container::ensure_running`] : `docker start` sur un conteneur pré-existant ne
    /// **réévalue jamais** les mounts. Cohérent avec le gel d'ADR-0031 §6 — un profil
    /// édité en cours de Run ne remonte rien — mais jusqu'ici non dit.
    pub extra_mounts: &'a [StagedMount],
    /// Racines de home des harnais first-party **hors `.claude`** (#708, ADR-0063 §3)
    /// — calculées par [`crate::domain::staging::harness_home_mounts`] : un
    /// `-v <staging>/home/.pi/agent:<host_home>/.pi/agent:rw` par staging set dont la
    /// racine n'est pas déjà un mount fixe. Montées **vides** au create, remplies au
    /// spawn du premier nœud qui résout le harnais (`fill_staging_set`). Posées juste
    /// après les 4 `-v` fixes, avant la queue du profil : ce sont des mounts **de
    /// PDO**, pas de l'utilisateur, et leur liste ne dépend que du registre embarqué.
    pub harness_homes: &'a [StagedMount],
    /// Env du profil de staging (#468, ADR-0031 §8) — la liste GELÉE à la création du Run,
    /// résolue au bord par `sandbox_run::frozen_env`. Queue **variable** comme
    /// [`ContainerSpec::extra_mounts`] : vide ⇒ argv byte-identique à #432 (propriété que
    /// le golden test pin explicitement).
    ///
    /// `BTreeMap` et non un slice de paires : une clé ne peut porter qu'UNE valeur (un
    /// `-e K=v1 -e K=v2` serait résolu différemment selon la couche qui lit `environ`), et
    /// l'ordre des clés est déterministe sans que le caller ait à trier — même raison que
    /// le tri de `sandbox_staging::extra_mounts`, qui rend le golden lisible.
    ///
    /// Mêmes remarques de gel que les mounts : `docker start` ne réévalue pas l'env d'un
    /// conteneur pré-existant, donc éditer l'env d'un profil pendant un Run vivant ne
    /// change rien pour ses nœuds suivants — c'est l'AC3 de #468, et c'est garanti deux
    /// fois (par le gel du payload ET par Docker).
    pub env: &'a BTreeMap<String, String>,
    /// `.git` hôte de chaque secondaire **modifiable** (ADR-0047), monté rw à
    /// chemin **identique** (`-v <g>:<g>:rw`, invariant D3). Le store d'objets
    /// d'un snapshot secondaire vit sous `<secondary>/.git`, hors `repo_root` :
    /// sans ce mount, `git status`/`commit` échouent en sandbox. `:rw` obligatoire
    /// (`:ro` casse l'index — EROFS). Queue **variable** posée après
    /// [`ContainerSpec::extra_mounts`] et **avant** l'image ; vide ⇒ argv
    /// byte-identique (secondaire read-only, mono-repo, ou aucun secondaire).
    pub writable_secondary_gitdirs: &'a [PathBuf],
}

/// Env posé au `docker create`, **fusionné** : les 4 vars run-constantes d'abord, puis
/// l'env du profil de staging (#468). Exactement UN couple par clé.
///
/// La fusion n'est pas de la coquetterie. Deux `-e` pour la même clé produisent deux
/// entrées dans `Config.Env`, et « laquelle gagne » dépend de la couche qui lit
/// l'environnement (`execve` conserve le tableau tel quel, `getenv` rend la première
/// occurrence, un shell la dernière). Un invariant d'isolation ne peut pas dépendre de ça :
/// on ne produit donc jamais de doublon.
///
/// Mesuré au passage sur Docker 29.2.1 : **l'ordre de `Config.Env` n'est pas celui de
/// l'argv** (un `-e FOO=bar` posé en 5e position ressort en 1re). Raison de plus pour ne
/// jamais raisonner en « le dernier gagne » — l'ordre n'est pas à nous.
///
/// Deux règles, et elles sont différentes exprès :
/// - une clé de [`RUN_CONSTANT_ENV_KEYS`] est **ignorée** (ceinture-bretelles : le PUT
///   l'a déjà refusée avec un 400 nommant la clé) ;
/// - [`CLAUDE_TRAFFIC_ENV`] est **remplacé en place** — même position dans l'argv, donc le
///   golden d'un profil qui le surcharge reste lisible.
///
/// Les clés inconnues sont ajoutées après le bloc fixe, dans l'ordre des clés du `BTreeMap`.
fn create_env(run_id: &str, spec: &ContainerSpec) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = vec![
        ("HOME".to_string(), spec.host_home.display().to_string()),
        (
            "PDO_DAEMON_URL".to_string(),
            daemon_url(spec.daemon_port, true),
        ),
        ("PDO_RUN_ID".to_string(), run_id.to_string()),
        (CLAUDE_TRAFFIC_ENV.to_string(), "1".to_string()),
    ];
    for (key, value) in spec.env {
        if RUN_CONSTANT_ENV_KEYS.contains(&key.as_str()) {
            continue;
        }
        match env.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value.clone(),
            None => env.push((key.clone(), value.clone())),
        }
    }
    env
}

/// Argv `docker create` (après le binaire `docker`). Ordre canonique FIGÉ par le golden test :
/// `--init`, `--name`, `--user` numérique, `--add-host`, `-w`, les `-e` fusionnés
/// ([`create_env`] : 4 fixes + la queue du profil, #468), les 4 `-v` **fixes**, les
/// racines de home des harnais first-party montées **vides** (#708 : `.pi/agent`),
/// puis la **queue variable** des mounts d'exception `$HOME` (#432), l'image, et le
/// trailing `sleep infinity` (le conteneur dort ; les tails entrent par `docker exec`).
///
/// La queue de mounts est **entre** les `-v` fixes et l'image — position forcée par Docker :
/// après l'image, tout argument devient la commande. Elle est triée par chemin relatif
/// ([`crate::adapters::staging::extra_mounts`]) : un ordre déterministe rend le golden
/// lisible, et le tri est ce qui garde le collapse des imbriqués linéaire. Docker lui-même
/// s'en moque (il applique les mounts par profondeur de destination, pas par ordre d'argv).
///
/// **Deux queues variables, deux goldens vides.** Un profil sans env ET sans entrée
/// d'exception `$HOME` rend un argv byte-identique à #406 : c'est ce qui garantit qu'aucune
/// des deux slices n'a touché les Runs qui ne les utilisent pas.
pub fn create_args(run_id: &str, spec: &ContainerSpec) -> Vec<String> {
    let mut args = vec![
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
        format!("{CONTAINER_SIDE_HOST}:host-gateway"),
        // -w au create (cosmétique, satisfait l'AC ; load-bearing = par-exec).
        "-w".to_string(),
        spec.run_worktree.display().to_string(),
    ];
    for (key, value) in create_env(run_id, spec) {
        args.push("-e".to_string());
        args.push(format!("{key}={value}"));
    }
    args.extend([
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
    ]);
    // Racines de home des harnais first-party (#708, ADR-0063 §3) : montées VIDES,
    // remplies au spawn. Après les 4 fixes (ce sont des mounts de PDO), avant la queue
    // du profil (celle de l'utilisateur).
    for mount in spec.harness_homes {
        args.push("-v".to_string());
        args.push(format!(
            "{}:{}:rw",
            mount.source.display(),
            mount.target.display()
        ));
    }
    // Queue variable (#432) : un `-v <source>:<target>:rw` par entrée d'exception
    // `$HOME` réellement stagée. Vide ⇒ l'argv reste byte-identique à #406.
    for mount in spec.extra_mounts {
        args.push("-v".to_string());
        args.push(format!(
            "{}:{}:rw",
            mount.source.display(),
            mount.target.display()
        ));
    }
    // Queue variable (ADR-0047) : le `.git` de chaque secondaire MODIFIABLE, monté
    // rw à chemin identique (host == conteneur, invariant D3). Après la queue #432
    // et avant l'image (tout ce qui suit l'image devient la commande). Vide ⇒ argv
    // byte-identique — un secondaire read-only n'en gagne aucun.
    for gitdir in spec.writable_secondary_gitdirs {
        args.push("-v".to_string());
        args.push(format!("{g}:{g}:rw", g = gitdir.display()));
    }
    args.push(spec.image_ref.to_string());
    args.push("sleep".to_string());
    args.push("infinity".to_string());
    args
}

/// Script `sh -c` qui donne une **identité nommée** à l'uid/gid hôte dans le conteneur, en
/// **ajoutant** au `/etc/passwd` et `/etc/group` réels de l'image — jamais en les remplaçant.
///
/// Le défaut qu'il ferme : un conteneur tourne en `--user <uid>:<gid>` numérique, et l'image
/// `ubuntu:24.04` ne connaît que l'uid 1000 (`ubuntu`). Pour tout autre uid, `sudo` appelle
/// `getpwuid()` avant d'appliquer NOPASSWD et abandonne sur « you do not exist in the passwd
/// database » — l'agent perd `apt install`, la raison même pour laquelle l'image embarque `sudo`.
///
/// Trois détails sont load-bearing, chacun mesuré :
/// - **`append`, pas `replace`** — `sudo` résout `root` PAR NOM avant tout le reste ; un passwd
///   réduit à notre ligne rend `sudo: unknown user root`. Un `/etc/group` amputé fait en plus
///   perdre à apt son bac à sable `_apt`.
/// - **champ mot de passe `*`, pas `x`** — avec `x`, la phase *account* de `pam_unix` va chercher
///   `/etc/shadow`, n'y trouve rien, et `sudo` rend « account validation failure ». `*` court-
///   circuite la recherche. C'est ce qui évite un TROISIÈME fichier à injecter.
/// - **garde `getent`** — c'est la conditionnalité, et elle est exacte : elle interroge l'image
///   réelle au lieu de supposer laquelle c'est. En uid 1000 elle résout (`ubuntu`) et rien n'est
///   écrit (fichiers byte-identiques) ; le chemin laptop courant est donc inchangé sans qu'aucun
///   `if uid != 1000` n'existe en Rust — pas de branche morte, un seul chemin testé. Elle rend
///   aussi le script **idempotent**, ce qu'exige un appel sur les trois branches d'
///   [`crate::application::container::ensure_running`] et à chaque `ensure_ready` du Run.
///
/// Home = le `$HOME` hôte, celui du `-e HOME=` du create : `getent`, `~` et l'environnement
/// doivent désigner le même répertoire (et c'est de là que Node lirait `userInfo().homedir` si
/// Claude Code repassait un jour sur une distribution Node).
pub fn identity_script(uid: u32, gid: u32, host_home: &Path) -> String {
    let passwd_line = format!(
        "{IDENTITY_NAME}:*:{uid}:{gid}:PDO sandbox:{}:/bin/bash",
        host_home.display()
    );
    let group_line = format!("{IDENTITY_NAME}:x:{gid}:");
    format!(
        "getent passwd {uid} >/dev/null 2>&1 || printf '%s\\n' {p} >> /etc/passwd; \
         getent group {gid} >/dev/null 2>&1 || printf '%s\\n' {g} >> /etc/group",
        p = sh_single_quote(&passwd_line),
        g = sh_single_quote(&group_line),
    )
}

/// Argv `docker exec` de l'injection d'identité (après le binaire `docker`). Ordre canonique
/// FIGÉ par le golden test.
///
/// **`--user 0:0`** : écrire dans `/etc/passwd` demande root, et le `--user` d'un `exec` écrase
/// celui du `create`. Le sandbox n'est pas une frontière de sécurité (ADR-0030) — le daemon
/// possède déjà le conteneur, il ne s'accorde rien de neuf.
///
/// **Aucun `-e`, aucun `-i`/`-t`, aucun `-w`** : ce n'est pas une session de nœud. En
/// particulier pas de [`super::SESSION_MARKER_ENV`], sinon le kill ciblé de la première session tuerait
/// aussi cet exec (même raison que pour [`crate::application::container::kill_session_in_container`]).
pub fn identity_exec_args(run_id: &str, uid: u32, gid: u32, host_home: &Path) -> Vec<String> {
    vec![
        "exec".to_string(),
        "--user".to_string(),
        "0:0".to_string(),
        container_name(run_id),
        "sh".to_string(),
        "-c".to_string(),
        identity_script(uid, gid, host_home),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::container::SESSION_MARKER_ENV;
    use std::process::Command;

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

        fn with_extras(mut self, rels: &[&str]) -> Self {
            self.extras = rels
                .iter()
                .map(|rel| StagedMount {
                    source: PathBuf::from(format!("/sb/r1/home/{rel}")),
                    target: PathBuf::from(format!("/home/u/{rel}")),
                })
                .collect();
            self
        }

        fn with_env(mut self, pairs: &[(&str, &str)]) -> Self {
            self.env = pairs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            self
        }

        fn with_writable_gitdirs(mut self, dirs: &[&str]) -> Self {
            self.writable_gitdirs = dirs.iter().map(PathBuf::from).collect();
            self
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

    /// L'anti-dérive : la valeur de `-e PDO_DAEMON_URL` posée au create N'EST PAS un
    /// littéral parallèle, c'est **le même appel** que celui du texte des préambules.
    /// Si ce test tombe, l'env et la prose ont recommencé à diverger — le bug #447.
    #[test]
    fn create_env_daemon_url_comes_from_the_shared_resolver() {
        let fx = Fixtures::sample();
        let args = create_args("r1", &fx.spec());
        let posed = args
            .iter()
            .find_map(|a| a.strip_prefix("PDO_DAEMON_URL="))
            .expect("le create pose PDO_DAEMON_URL");
        assert_eq!(posed, daemon_url(6172, true));
    }

    /// Le hostname de la gateway que le create **crée** (`--add-host`) est le même que
    /// celui que le résolveur **rend**. Deux littéraux désynchronisés donneraient un
    /// `PDO_DAEMON_URL` pointant vers un nom que le conteneur ne résout pas.
    #[test]
    fn add_host_and_resolver_name_the_same_gateway() {
        let fx = Fixtures::sample();
        let args = create_args("r1", &fx.spec());
        let mapping = args
            .iter()
            .find(|a| a.ends_with(":host-gateway"))
            .expect("le create pose --add-host");
        let host = mapping.trim_end_matches(":host-gateway");
        assert!(
            daemon_url(6172, true).contains(host),
            "--add-host {host} n'est pas le hostname du résolveur ({})",
            daemon_url(6172, true)
        );
    }

    // « Accommoder la queue variable, pas la figer » : le préfixe FIXE est gravé UNE
    // fois, chaque queue est prouvée séparément, et un test pin la propriété
    // structurelle (les extras ne peuvent QUE grossir la queue). Le filtre de dédup se
    // teste là où il vit — `sandbox_staging::extra_mounts` — pas à travers `create_args`.
    //
    // #468 coupe le préfixe en DEUX morceaux (`…_head` = flags + les 4 `-e` fixes,
    // `…_mounts` = les 4 `-v` fixes) parce qu'une deuxième queue variable s'insère entre
    // les deux. Un profil sans env recolle les deux et retrouve l'argv de #432, ce que
    // `create_argv_fixed` (conservé, reconstitué) exprime en une ligne.

    /// L'argv jusqu'aux 4 `-e` run-constants inclus. FIGÉ ICI, et nulle part ailleurs.
    fn create_argv_head() -> Vec<String> {
        strings(&[
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
        ])
    }

    /// Les 4 `-v` fixes (identity mounts) + la racine de home de `pi` montée vide
    /// (#708, ADR-0063 §3 — `.claude` est déjà le mount `claude-home`).
    fn create_argv_mounts() -> Vec<String> {
        strings(&[
            "-v",
            "/repo:/repo:rw",
            "-v",
            "/sb/r1/claude-home:/home/u/.claude:rw",
            "-v",
            "/sb/r1/.claude.json:/home/u/.claude.json:rw",
            "-v",
            "/host/bin/pdo:/usr/local/bin/pdo:ro",
            "-v",
            "/sb/r1/home/.pi/agent:/home/u/.pi/agent:rw",
        ])
    }

    /// Le préfixe fixe d'un profil **sans env** : head + mounts, sans rien entre les deux.
    fn create_argv_fixed() -> Vec<String> {
        let mut v = create_argv_head();
        v.extend(create_argv_mounts());
        v
    }

    /// L'image + la commande dormante.
    fn create_argv_suffix() -> Vec<String> {
        strings(&["pdo-sandbox:h-abc123", "sleep", "infinity"])
    }

    /// Queue vide ⇒ argv **byte-identique** à #406. La propriété qui garantit que #432
    /// n'a touché aucun Run dont le profil n'a pas d'entrée d'exception `$HOME`.
    #[test]
    fn create_args_golden_no_extras() {
        let fx = Fixtures::sample();
        let mut expected = create_argv_fixed();
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// #708 : la racine de home d'un harnais first-party est montée VIDE, juste après
    /// les 4 `-v` fixes et AVANT la queue du profil. Sans aucune racine (un registre
    /// où seul `claude` déclare un set), l'argv est byte-identique à celui d'avant.
    #[test]
    fn harness_home_roots_sit_between_the_fixed_mounts_and_the_profile_queue() {
        let mut fx = Fixtures::sample().with_extras(&[".gitconfig"]);
        let with = create_args("r1", &fx.spec());
        let pi_at = with
            .iter()
            .position(|a| a == "/sb/r1/home/.pi/agent:/home/u/.pi/agent:rw")
            .expect("pi's home root is mounted");
        let pdo_at = with
            .iter()
            .position(|a| a == "/host/bin/pdo:/usr/local/bin/pdo:ro")
            .unwrap();
        let gitconfig_at = with
            .iter()
            .position(|a| a == "/sb/r1/home/.gitconfig:/home/u/.gitconfig:rw")
            .unwrap();
        assert!(pdo_at < pi_at && pi_at < gitconfig_at, "{with:?}");

        fx.harness_homes.clear();
        let without = create_args("r1", &fx.spec());
        assert_eq!(without.len(), with.len() - 2, "one root = exactly 2 args");
        assert!(
            !without.iter().any(|a| a.contains(".pi/agent")),
            "no set ⇒ no pi mount: {without:?}"
        );
    }

    /// Une liste précise → une queue précise, insérée entre les `-v` fixes et l'image.
    #[test]
    fn create_args_golden_with_extras() {
        let fx = Fixtures::sample().with_extras(&[".gitconfig", ".config/gh"]);
        let mut expected = create_argv_fixed();
        expected.extend(strings(&[
            "-v",
            "/sb/r1/home/.gitconfig:/home/u/.gitconfig:rw",
            "-v",
            "/sb/r1/home/.config/gh:/home/u/.config/gh:rw",
        ]));
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// La propriété STRUCTURELLE, indépendante des chemins : le préfixe est intact, la
    /// queue est insérée avant l'image, et une entrée coûte exactement deux args.
    /// Position forcée par Docker — après l'image, tout devient la commande.
    #[test]
    fn extras_only_grow_the_tail_between_the_fixed_v_and_the_image() {
        let fixed = create_argv_fixed();
        let bare = create_args("r1", &Fixtures::sample().spec());
        let one = create_args(
            "r1",
            &Fixtures::sample().with_extras(&[".gitconfig"]).spec(),
        );

        assert_eq!(
            &one[..fixed.len()],
            &fixed[..],
            "le préfixe fixe est intact"
        );
        assert_eq!(one.len(), bare.len() + 2, "une entrée = exactement 2 args");
        // La queue précède l'image, qui précède la commande.
        let image_at = one
            .iter()
            .position(|a| a == "pdo-sandbox:h-abc123")
            .unwrap();
        assert_eq!(&one[image_at..], &create_argv_suffix()[..]);
        assert_eq!(one[image_at - 2], "-v");
    }

    /// Un secondaire modifiable ⇒ un `-v <g>:<g>:rw` (chemin identique host==conteneur),
    /// inséré entre les `-v` fixes/extras et l'image. Un read-only n'en gagne aucun (il
    /// n'entre jamais dans `writable_secondary_gitdirs`, cf. `context_from_state`).
    #[test]
    fn create_args_golden_with_writable_secondary_gitdir() {
        let fx = Fixtures::sample().with_writable_gitdirs(&["/work/sdk/.git"]);
        let mut expected = create_argv_fixed();
        expected.extend(strings(&["-v", "/work/sdk/.git:/work/sdk/.git:rw"]));
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// Aucun secondaire modifiable ⇒ argv byte-identique (la propriété qui garantit
    /// qu'un Run mono-repo ou tout-read-only n'est pas touché par ADR-0047).
    #[test]
    fn create_args_without_writable_gitdirs_is_byte_identical() {
        let fx = Fixtures::sample().with_writable_gitdirs(&[]);
        let mut expected = create_argv_fixed();
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// Ordre des deux queues variables : les mounts d'exception `$HOME` (#432) d'abord,
    /// puis les `.git` modifiables (ADR-0047), puis l'image. Deux `.git` → deux entrées.
    #[test]
    fn writable_gitdirs_follow_extras_and_precede_the_image() {
        let fx = Fixtures::sample()
            .with_extras(&[".gitconfig"])
            .with_writable_gitdirs(&["/work/a/.git", "/work/b/.git"]);
        let mut expected = create_argv_fixed();
        expected.extend(strings(&[
            "-v",
            "/sb/r1/home/.gitconfig:/home/u/.gitconfig:rw",
            "-v",
            "/work/a/.git:/work/a/.git:rw",
            "-v",
            "/work/b/.git:/work/b/.git:rw",
        ]));
        expected.extend(create_argv_suffix());
        let got = create_args("r1", &fx.spec());
        assert_eq!(got, expected);
        // Structurel : chaque `.git` précède l'image, et coûte exactement 2 args.
        let image_at = got
            .iter()
            .position(|a| a == "pdo-sandbox:h-abc123")
            .unwrap();
        assert_eq!(&got[image_at..], &create_argv_suffix()[..]);
        assert_eq!(got[image_at - 2], "-v");
    }

    /// AC#5 de #468, dit deux fois : pas d'env ⇒ l'argv de #432, au byte.
    /// `create_args_golden_no_extras` le prouve déjà pour la queue de mounts ; celui-ci
    /// nomme la propriété côté env, pour qu'un futur « et si on posait toujours un `-e`
    /// vide ? » tombe sur un test qui dit non.
    #[test]
    fn create_args_without_env_is_byte_identical() {
        let fx = Fixtures::sample().with_env(&[]);
        let mut expected = create_argv_fixed();
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// AC#1 : `env: {FOO: bar}` ⇒ un `-e FOO=bar` au create. La queue s'insère entre les
    /// 4 `-e` fixes et les `-v`, et les clés sortent triées (ordre du `BTreeMap`).
    #[test]
    fn create_args_golden_with_env() {
        let fx = Fixtures::sample().with_env(&[
            ("PUPPETEER_EXECUTABLE_PATH", "/usr/bin/chromium"),
            ("FOO", "bar"),
        ]);
        let mut expected = create_argv_head();
        expected.extend(strings(&[
            "-e",
            "FOO=bar",
            "-e",
            "PUPPETEER_EXECUTABLE_PATH=/usr/bin/chromium",
        ]));
        expected.extend(create_argv_mounts());
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// Les deux queues variables sont INDÉPENDANTES et gardent chacune sa place : env
    /// avant les `-v` fixes, mounts après. Une inversion casserait le golden de #432 sans
    /// que rien d'autre ne le signale.
    #[test]
    fn env_and_mount_tails_keep_their_own_slots() {
        let fx = Fixtures::sample()
            .with_env(&[("FOO", "bar")])
            .with_extras(&[".gitconfig"]);
        let mut expected = create_argv_head();
        expected.extend(strings(&["-e", "FOO=bar"]));
        expected.extend(create_argv_mounts());
        expected.extend(strings(&[
            "-v",
            "/sb/r1/home/.gitconfig:/home/u/.gitconfig:rw",
        ]));
        expected.extend(create_argv_suffix());
        assert_eq!(create_args("r1", &fx.spec()), expected);
    }

    /// Ceinture-bretelles : même si une clé réservée arrivait jusqu'ici (payload gelé
    /// bricolé à la main, régression du validateur du PUT), le create ne la re-pose PAS.
    /// Un `HOME` écrasé casserait les deux mounts `.claude` / `.claude.json` d'un coup.
    #[test]
    fn create_args_never_lets_a_profile_clobber_a_run_constant() {
        let fx = Fixtures::sample().with_env(&[
            ("HOME", "/tmp/evil"),
            ("PDO_DAEMON_URL", "http://localhost:1"),
            ("PDO_RUN_ID", "not-r1"),
            ("SAFE", "kept"),
        ]);
        let args = create_args("r1", &fx.spec());
        // Le bloc fixe est intact, et la seule clé non réservée a survécu.
        let mut expected = create_argv_head();
        expected.extend(strings(&["-e", "SAFE=kept"]));
        expected.extend(create_argv_mounts());
        expected.extend(create_argv_suffix());
        assert_eq!(args, expected);
        // Aucune clé run-constante n'apparaît deux fois : « laquelle gagne » ne doit
        // jamais dépendre de la couche qui lit `environ`.
        for key in RUN_CONSTANT_ENV_KEYS {
            let posed = args
                .iter()
                .filter(|a| a.starts_with(&format!("{key}=")))
                .count();
            assert_eq!(
                posed, 1,
                "`{key}` doit être posé exactement une fois: {args:?}"
            );
        }
    }

    /// `CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC` n'est PAS réservé : un profil peut le
    /// surcharger, et la valeur est remplacée **en place** (un seul `-e`, position
    /// inchangée) — pas un second `-e` dont l'effet dépendrait du lecteur d'`environ`.
    #[test]
    fn a_profile_may_override_the_claude_traffic_knob_in_place() {
        let fx = Fixtures::sample().with_env(&[(CLAUDE_TRAFFIC_ENV, "0")]);
        let args = create_args("r1", &fx.spec());
        let mut expected = create_argv_head();
        // Même longueur que le head par défaut : la valeur a été remplacée sur place.
        let slot = expected.len() - 1;
        expected[slot] = format!("{CLAUDE_TRAFFIC_ENV}=0");
        expected.extend(create_argv_mounts());
        expected.extend(create_argv_suffix());
        assert_eq!(args, expected);
        assert!(
            !RUN_CONSTANT_ENV_KEYS.contains(&CLAUDE_TRAFFIC_ENV),
            "le knob Claude Code est surchargeable par construction"
        );
    }

    /// L'anti-dérive de #468, jumelle de `create_env_daemon_url_comes_from_the_shared_resolver` :
    /// toute clé que `sandbox_profile` refuse est bien une clé que le create POSE. Si
    /// quelqu'un retire un `-e` du create sans retirer la clé de la liste, le PUT
    /// continuerait à refuser une clé que plus rien ne protège.
    ///
    /// Unidirectionnel exprès : [`CLAUDE_TRAFFIC_ENV`] est posé sans être réservé.
    #[test]
    fn every_reserved_key_is_actually_posed_by_the_create() {
        let fx = Fixtures::sample();
        let args = create_args("r1", &fx.spec());
        for key in RUN_CONSTANT_ENV_KEYS {
            assert!(
                args.iter().any(|a| a.starts_with(&format!("{key}="))),
                "`{key}` est réservé au PUT mais le create ne le pose pas: {args:?}"
            );
        }
    }

    // Le script est le seul endroit du dépôt où vivent les subtilités mesurées pendant le
    // grilling (`*` vs `x`, `>>` vs `>`, la garde). Chacune a son test NOMMÉ, pour qu'une
    // simplification bien intentionnée tombe sur un refus qui porte sa raison.

    /// Le golden : la chaîne EXACTE pour un uid hôte ≠ 1000. C'est ce texte que l'humain de
    /// la HP recopie pour reproduire à la main (§3.5 du plan #414).
    #[test]
    fn identity_script_golden() {
        assert_eq!(
            identity_script(1234, 1234, Path::new("/home/u")),
            "getent passwd 1234 >/dev/null 2>&1 || printf '%s\\n' \
             'pdo:*:1234:1234:PDO sandbox:/home/u:/bin/bash' >> /etc/passwd; \
             getent group 1234 >/dev/null 2>&1 || printf '%s\\n' 'pdo:x:1234:' >> /etc/group"
        );
    }

    /// D4 — le champ mot de passe de la ligne passwd est `*`, JAMAIS `x`. Avec `x`, la phase
    /// *account* de `pam_unix` cherche la ligne dans `/etc/shadow`, ne la trouve pas, et `sudo`
    /// rend « account validation failure, is your account locked? » : il faudrait alors injecter
    /// un TROISIÈME fichier. La ligne `group`, elle, porte bien `x` (c'est son champ mot de passe
    /// de groupe, sans rapport avec shadow) — d'où la tentation d'« harmoniser », que ce test
    /// refuse en nommant la raison.
    #[test]
    fn identity_script_password_field_is_star_not_x() {
        let script = identity_script(1234, 5678, Path::new("/home/u"));
        assert!(
            script.contains("'pdo:*:1234:5678:"),
            "la ligne passwd doit porter `*` comme champ mot de passe: {script}"
        );
        assert!(
            !script.contains("'pdo:x:1234:"),
            "un `x` en passwd renverrait PAM vers /etc/shadow (account validation failure): {script}"
        );
        // Contrepartie : la ligne de groupe garde son `x`, c'est sa forme canonique.
        assert!(script.contains("'pdo:x:5678:'"), "ligne group: {script}");
    }

    /// D5 — on AJOUTE, on ne tronque jamais. Un `/etc/passwd` réduit à notre ligne rend
    /// `sudo: unknown user root` (sudo résout `root` PAR NOM avant tout le reste), et un
    /// `/etc/group` amputé fait perdre à apt son bac à sable `_apt`. Les seuls `>` tolérés
    /// sont les deux `>>` et les redirections `>/dev/null` de la garde.
    #[test]
    fn identity_script_appends_and_never_truncates() {
        let script = identity_script(1234, 1234, Path::new("/home/u"));
        assert_eq!(
            script.matches(">>").count(),
            2,
            "exactement deux appends (passwd + group): {script}"
        );
        let stripped = script
            .replace(">> /etc/passwd", "")
            .replace(">> /etc/group", "");
        // Les redirections de la GARDE sont légitimes : elles muselent `getent`, elles
        // n'écrivent aucun fichier.
        let stripped = stripped.replace(">/dev/null 2>&1", "");
        assert!(
            !stripped.contains('>'),
            "aucune redirection tronquante ne doit exister: {script}"
        );
        for verb in ["tee", "sed -i", "truncate", "cat >"] {
            assert!(
                !script.contains(verb),
                "`{verb}` réécrirait le fichier au lieu de l'étendre: {script}"
            );
        }
    }

    /// Un `$HOME` hôte hostile (apostrophe + espace) reste UN seul argument `sh` correctement
    /// quoté. Prouvé en faisant round-tripper le littéral par un vrai `sh`, pas en relisant le
    /// quoting à l'œil.
    #[test]
    fn identity_script_quotes_a_hostile_home() {
        let home = Path::new("/home/o'brien x");
        let script = identity_script(1234, 1234, home);
        let line = "pdo:*:1234:1234:PDO sandbox:/home/o'brien x:/bin/bash";
        let quoted = sh_single_quote(line);
        assert!(
            script.contains(&quoted),
            "la ligne passwd doit être single-quotée en entier: {script}"
        );
        let out = Command::new("sh")
            .arg("-c")
            .arg(format!("printf '%s' {quoted}"))
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&out.stdout),
            line,
            "le quoting doit round-tripper par un vrai `sh`"
        );
    }

    /// Golden argv de l'injection : root (`--user 0:0`, écrire `/etc/passwd` l'exige) et
    /// **aucun** marqueur de session — sinon le kill ciblé de la première session emporterait
    /// aussi cet exec (même raison que pour le kill exec lui-même).
    #[test]
    fn identity_exec_runs_as_root_without_a_session_marker() {
        let args = identity_exec_args("r1", 1234, 1234, Path::new("/home/u"));
        let mut expected = strings(&["exec", "--user", "0:0", "pdo-sbx-r1", "sh", "-c"]);
        expected.push(identity_script(1234, 1234, Path::new("/home/u")));
        assert_eq!(args, expected);
        assert!(
            !args.iter().any(|a| a.contains(SESSION_MARKER_ENV)),
            "pas de marqueur de session sur l'exec d'identité: {args:?}"
        );
        // Ce n'est pas une session de nœud : ni tty, ni forwarding d'env, ni cwd.
        for flag in ["-i", "-t", "-e", "-w"] {
            assert!(
                !args.iter().any(|a| a == flag),
                "`{flag}` n'a rien à faire sur l'exec d'identité: {args:?}"
            );
        }
    }

    /// D1 — l'identité n'est PAS un mount : l'argv du `create` ne porte ni `/etc/passwd` ni
    /// `/etc/group`. Les goldens de `create_args` le prouvent déjà par égalité ; celui-ci
    /// **nomme** la propriété, parce que le mécanisme prescrit par l'issue était précisément
    /// un bind-mount et qu'il casse `useradd`/`apt` (en `:ro` comme en `:rw`).
    #[test]
    fn create_args_is_untouched_by_identity_injection() {
        let fx = Fixtures::sample();
        let args = create_args("r1", &fx.spec());
        for forbidden in ["/etc/passwd", "/etc/group", "/etc/shadow"] {
            assert!(
                !args.iter().any(|a| a.contains(forbidden)),
                "`{forbidden}` ne doit jamais être monté: {args:?}"
            );
        }
        // Le `--user` du create reste NUMÉRIQUE même une fois passwd peuplé : `--user <nom>`
        // ferait résoudre le gid primaire via passwd → gid 0 (le bug que #406 ferme).
        let user_at = args.iter().position(|a| a == "--user").unwrap();
        assert_eq!(args[user_at + 1], "1000:1000");
    }
}
