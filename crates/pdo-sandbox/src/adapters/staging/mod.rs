//! Per-run sandbox home-staging on the local filesystem (#404, slice A du PRD #403) —
//! the one implementation of [`HomeStaging`].
//!
//! Zéro Docker. Pas d'`AppState`, pas d'async, pas de lecture d'env — seulement
//! `&Path` / `&str` in, `std::fs` out ; la path-math vit dans
//! [`crate::domain::staging`].
//!
//! Ce module gère le cycle de vie du *staged Claude home* d'un Run sandboxé :
//! [`prepare`] (seeder), [`merge_back`] (récupérer les transcripts), [`teardown`]
//! (purger).
//!
//! ## Décisions de conception (voir la section « Sandbox » de `CONTEXT.md`)
//! - **Allowlist, jamais denylist.** Copier « tout `~/.claude` sauf `projects/` »
//!   embarquerait tout l'état hôte transitoire (`history.jsonl`, `session-env/`,
//!   `file-history/`…) — fuite d'isolation + fragile aux futures versions de Claude
//!   Code. On copie une liste explicite. Depuis #432 cette liste est **nommée et
//!   éditable** : c'est le *profil de staging* ([`crate::domain::profile`]), résolu et
//!   **gelé** dans `RunStarted`. Ce module ne connaît plus de « mode » — il reçoit une
//!   liste d'entrées, et la vide (`minimal`) est un no-op de phase 1.
//! - **Volume assumé ~1 Go/run** (#409, mesuré ; dominé par `plugins/*/node_modules`,
//!   requis par les serveurs MCP *dans* le conteneur — délibérément non strippés).
//!   Dette disque : le staging n'est purgé qu'au `cleanup_run` → à surveiller au
//!   regard de la récurrence disque connue (recette janitor pour l'usage massif).
//! - **Symlinks échappants déréférencés** (#409). Une part notable des skills sont
//!   des liens relatifs vers `~/.agents` (hors `~/.claude`, ni copié ni monté) :
//!   recréés verbatim ils dangleraient dans le conteneur (skills invisibles).
//!   `copy_tree_preserving` copie donc le CONTENU réel des liens qui **sortent** de
//!   l'arbre `~/.claude` ; les liens **intra-arbre** (cycles `node_modules/.bin`)
//!   restent des liens. Walk **best-effort par entrée** : un `~/.claude` volumineux
//!   que d'autres process Claude mutent ne doit jamais faire échouer le Run.
//! - **`merge_back` récurse.** ~42 % des transcripts vivent dans
//!   `projects/<enc>/<uuid>/subagents/*.jsonl` (profondeur 9). Le copy-set doit
//!   *égaler* le read-set du coût (`collect_jsonl_recursive`),
//!   sinon le coût des runs sandboxés est sous-estimé (régression silencieuse).
//! - **Le staging set d'un harnais est profil-agnostique et se remplit au spawn**
//!   (#426, ADR-0031 §1 ; #708, ADR-0063 §3). [`prepare`] matérialise les entrées du
//!   profil et **ancre** les mounts ([`ensure_mount_anchors`]) : la racine de home de
//!   chaque harnais first-party (`StagingSet::home_root`) existe **vide**, le
//!   `.claude.json` sibling existe (objet vide) — un mount dont la source manque
//!   ferait créer par Docker un répertoire root-owned. Le set lui-même —
//!   [`fill_staging_set`] : entrées copiées de l'hôte, puits de transcripts créés
//!   vides, *autonomy fixups* synthétisés — est posé **au spawn de la première
//!   session du Run qui résout le harnais**, gardé par un marqueur par `(Run,
//!   harnais)`. Un Run claude-only n'a jamais l'auth de `pi` dans son conteneur ; un
//!   nœud repris ne re-copie pas ; un nœud ré-épinglé à chaud obtient son set. Pour
//!   `claude`, ce sont les **cinq garanties** d'ADR-0031 §1, octet pour octet :
//!   chacune satisfaite soit par une copie de l'hôte, soit par une synthèse de repli.
//!   Les fixups qui désarment un dialogue bloquant (confiance, bypass permissions) ne
//!   sont pas cosmétiques : un agent non surveillé se figerait dessus. Ce module est
//!   **l'interprète unique** d'un set ; il n'en déclare aucun.
//! - **Une entrée hors `.claude` est copiée puis montée** (#432, ADR-0031 §4) :
//!   `<staging>/home/<rel>` → `$HOME/<rel>` en rw, **jamais** un bind direct du
//!   fichier hôte. Un `git config --global` du conteneur touche la copie, pas le
//!   `~/.gitconfig` de l'utilisateur. Une entrée **sous** `.claude/` ne reçoit aucun
//!   `-v` propre — conséquence du classificateur unique
//!   [`crate::domain::profile::landing`], partagé par [`prepare`] et [`extra_mounts`].

mod copy;
mod set;
pub mod stores;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tracing::info;

use crate::application::ports::HomeStaging;
use crate::domain::profile::{landing, Landing};
use crate::domain::staging::{
    staged_claude_home, staged_claude_json, staged_home_extras, staged_path, staging_dir_for_run,
    staging_set_marker, StagedMount, StagingFill, CLAUDE_HOME_REL,
};

use copy::{copy_jsonl_tree, materialise_entries};
use set::{apply_staging_set, set_mode_0600};

/// The staged homes under a sandbox root, on the local filesystem.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsStaging;

impl HomeStaging for FsStaging {
    fn exists(&self, sandbox_root: &Path, run_id: &str) -> bool {
        staging_dir_for_run(sandbox_root, run_id).exists()
    }

    fn prepare(
        &self,
        home_root: &Path,
        sandbox_root: &Path,
        entries: &[String],
        run_id: &str,
    ) -> Result<PathBuf> {
        prepare(home_root, sandbox_root, entries, run_id)
    }

    fn ensure_mount_anchors(&self, sandbox_root: &Path, run_id: &str) -> Result<()> {
        ensure_mount_anchors(sandbox_root, run_id)
    }

    fn extra_mounts(
        &self,
        sandbox_root: &Path,
        run_id: &str,
        host_home: &Path,
        entries: &[String],
    ) -> Vec<StagedMount> {
        extra_mounts(sandbox_root, run_id, host_home, entries)
    }

    fn merge_back(&self, home_root: &Path, sandbox_root: &Path, run_id: &str) -> Result<()> {
        merge_back(home_root, sandbox_root, run_id)
    }

    fn teardown(&self, sandbox_root: &Path, run_id: &str) -> Result<()> {
        teardown(sandbox_root, run_id)
    }
}

/// La queue variable de `-v` qu'une liste d'entrées gelée produit (#432).
///
/// **Pas** une valeur de retour de [`prepare`] : `ensure_ready` saute `prepare` quand
/// le staging existe déjà, ce qui est le cas de 3 de ses 4 appelants. Dériver de
/// `liste gelée × disque` est **total** — même réponse que `prepare` ait tourné ou
/// non.
///
/// Deux filtres, dans cet ordre :
/// 1. `landing()` == [`crate::domain::profile::Landing::HomeExtra`] — une entrée sous
///    `.claude/` est déjà servie par le mount fixe `.claude` et ne reçoit **aucun**
///    `-v` propre (dédup obligatoire d'ADR-0031 §4) ;
/// 2. **règle M1** — la source doit exister sur disque. Un `-v` dont la source hôte
///    manque fait créer par Docker un répertoire `root:root 0755`, ce qui en escalade
///    (a) plante `git` quand un mount de *fichier* devient un *répertoire* par-dessus
///    `$HOME/.gitconfig`, (b) rend le mount inscriptible par personne, et (c) pour un
///    chemin multi-segment (`.config/gh`) crée `<staging>/home/.config` en root — ce
///    qui fait échouer le `remove_dir_all` de [`teardown`] en EACCES, erreur avalée
///    par son `let _ =`, laissant un staging de ~1 Go **définitivement indélébile par
///    le daemon**. Ça alimente la récurrence disque connue, silencieusement.
///
/// Tri par chemin relatif (déterministe pour le golden) ; les imbriqués sont déjà
/// collapsés à la résolution, la dédup ici est ceinture-bretelles.
pub fn extra_mounts(
    sandbox_root: &Path,
    run_id: &str,
    host_home: &Path,
    entries: &[String],
) -> Vec<StagedMount> {
    let extras_root = staged_home_extras(sandbox_root, run_id);
    let mut rels: Vec<&str> = entries
        .iter()
        .filter_map(|entry| match landing(entry) {
            Landing::HomeExtra { rel } => Some(rel),
            _ => None,
        })
        .collect();
    rels.sort_unstable();
    rels.dedup();
    rels.into_iter()
        .filter_map(|rel| {
            let source = extras_root.join(rel);
            // M1: only what was REALLY staged gets mounted.
            if !source.exists() {
                return None;
            }
            Some(StagedMount {
                source,
                target: host_home.join(rel),
            })
        })
        .collect()
}

/// Seede le staging d'un Run et renvoie sa racine (`<sandbox_root>/<run_id>`).
///
/// **Deux gestes** (#432 ; #708, ADR-0063 §3) :
/// 1. *matérialisation du profil* — une passe sur la liste d'entrées **gelée** ;
/// 2. *[`ensure_mount_anchors`]* — la racine de home de chaque harnais first-party
///    existe **vide**, le `.claude.json` sibling existe : ce sont les sources des
///    mounts fixes du `docker create`, qui doivent exister **avant** le conteneur.
///
/// Le staging set d'un harnais n'est **plus** posé ici : [`fill_staging_set`] le
/// copie au spawn de la première session du Run qui résout ce harnais. Ce que le
/// conteneur contient est exactement ce que le Run a résolu — un Run claude-only
/// n'a jamais l'auth de `pi`. Le fixup de `claude` reste *merge-aware* et non
/// profil-aware : quand l'entrée `settings.json` est cochée, le profil la copie ici
/// et le fixup la merge au spawn — ce double write est **voulu**.
///
/// `entries` (#432) occupe le slot de l'ancien `mode` : c'est la liste **résolue et
/// gelée** dans `RunStarted`, jamais le réglage vivant (ADR-0031 §6). l'ancien `Mode`
/// a disparu — les défauts virtuels sont des listes nommées, `minimal` étant la vide.
///
/// Idempotent (`create_dir_all` ; copy-or-overwrite) et **additif** — jamais de
/// suppression (ADR-0031 §6).
pub fn prepare(
    home_root: &Path,
    sandbox_root: &Path,
    entries: &[String],
    run_id: &str,
) -> Result<PathBuf> {
    let staging = staging_dir_for_run(sandbox_root, run_id);
    let home = staged_claude_home(sandbox_root, run_id);
    std::fs::create_dir_all(&home)
        .with_context(|| format!("create staged claude home {}", home.display()))?;

    let src = home_root.join(".claude");
    let staged_json = staged_claude_json(sandbox_root, run_id);

    materialise_entries(home_root, &src, &home, &staged_json, &staging, entries)?;
    ensure_mount_anchors(sandbox_root, run_id)?;

    Ok(staging)
}

/// Les **sources des mounts fixes** existent, vides quand rien ne les a remplies
/// (#708, ADR-0063 §3) : la racine de home de **chaque** staging set déclaré
/// ([`StagingSet::home_root`] → [`staged_path`]) et le `.claude.json` sibling (un
/// mount de *fichier* : absent, Docker créerait un **répertoire** root-owned à sa
/// place et Claude Code ne pourrait plus l'écrire — synthétisé en objet vide `{}`
/// 0600, que le fixup [`AutonomyFixup::ClaudeJsonBaseline`] complète au spawn).
///
/// Idempotent et additif ; appelé par [`prepare`] **et** par chaque `ensure_ready`
/// (un staging préparé par un daemon d'avant #708 n'a pas la racine `pi` : la créer
/// avant le `docker create` évite le répertoire root-owned de la règle M1 de
/// [`extra_mounts`]).
pub fn ensure_mount_anchors(sandbox_root: &Path, run_id: &str) -> Result<()> {
    for (harness, set) in pdo_harness::probes::staging_sets() {
        // `claude`'s `.claude` home is the fixed `claude-home` mount, seeded whole by
        // the profile — it needs no separate anchor. `landing(".claude")` classifies
        // bare `.claude` as a HomeExtra (only `.claude/` is stripped), so guard on the
        // exact name rather than the classifier.
        if set.home_root == CLAUDE_HOME_REL {
            continue;
        }
        let root = staged_path(sandbox_root, run_id, set.home_root);
        std::fs::create_dir_all(&root).with_context(|| {
            format!(
                "create the empty staged {harness} home root {}",
                root.display()
            )
        })?;
    }
    let staged_json = staged_claude_json(sandbox_root, run_id);
    if !staged_json.exists() {
        std::fs::write(&staged_json, "{}")
            .with_context(|| format!("anchor staged .claude.json {}", staged_json.display()))?;
        set_mode_0600(&staged_json)?;
    }
    Ok(())
}

/// Pose le staging set de `harness` dans le staging du Run **si ce Run ne l'a pas
/// encore** (#708, ADR-0063 §3) : la première session du Run qui résout ce harnais
/// copie le set ([`apply_staging_set`]) puis écrit le marqueur ; toute session
/// suivante du même harnais — un second nœud, un nœud repris, un nœud ré-épinglé —
/// trouve le marqueur et ne recopie rien. Un harnais sans set rend
/// [`StagingFill::NoSet`] sans toucher le disque.
///
/// Écrit exclusivement dans le staging (jamais l'hôte, jamais le conteneur : la
/// copie atterrit sous la racine de home montée vide par [`ensure_mount_anchors`],
/// donc elle est visible dans le conteneur sans recréation). `trusted_root` : la
/// racine à pré-approuver par le fixup `claude` (le `repo_root` effectif du Run).
///
/// **Fail-fast** : une garantie intenable doit échouer au spawn (le nœud passe
/// `Interrupted`, ADR-0049), pas produire une session qui pend sur un dialogue.
pub fn fill_staging_set(
    home_root: &Path,
    sandbox_root: &Path,
    run_id: &str,
    harness: &str,
    trusted_root: Option<&Path>,
) -> Result<StagingFill> {
    let Some(set) = pdo_harness::probes::probes_for(harness).and_then(|p| p.staging_set()) else {
        return Ok(StagingFill::NoSet);
    };
    let marker = staging_set_marker(sandbox_root, run_id, harness);
    if marker.exists() {
        return Ok(StagingFill::AlreadyFilled(set));
    }
    apply_staging_set(home_root, sandbox_root, run_id, harness, &set, trusted_root)?;
    if let Some(parent) = marker.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create staging set markers dir {}", parent.display()))?;
    }
    std::fs::write(&marker, "")
        .with_context(|| format!("write staging set marker {}", marker.display()))?;
    info!(
        "sandbox: run {run_id} staged the `{harness}` set at spawn ({} entr{}, {} transcript sink{}, {} fixup{}, env: {})",
        set.entries.len(),
        if set.entries.len() == 1 { "y" } else { "ies" },
        set.transcripts.len(),
        if set.transcripts.len() == 1 { "" } else { "s" },
        set.fixups.len(),
        if set.fixups.len() == 1 { "" } else { "s" },
        if set.env.is_empty() {
            "none".to_string()
        } else {
            set.env
                .iter()
                .map(|(k, _)| *k)
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    Ok(StagingFill::Filled(set))
}

/// Récupère les transcripts (`**/*.jsonl`) de **chaque puits déclaré par un staging
/// set** ([`pdo_harness::probes::staging_sets`] ; `claude` : `.claude/projects`,
/// `pi` : `.pi/agent/sessions`) du staging vers `<home_root>/<puits>/`,
/// **récursivement** (transcripts de sessions *et* de sous-agents
/// `<uuid>/subagents/*.jsonl`), sous le même dirname encodé — pour `pi`, le nom du
/// dossier dérive du cwd, identique dedans et dehors puisque le worktree est monté à
/// son chemin absolu hôte (ADR-0030 ; ADR-0063 « limites »). Générique : aucun
/// chemin de harnais n'est codé ici ; un puits jamais rempli (Run sans nœud de ce
/// harnais) est un no-op.
///
/// Idempotent : copie ssi le fichier hôte est **absent** OU **strictement plus
/// petit** (transcripts append-only ⇒ `staging > hôte ⇔ contenu nouveau`). Ne
/// réécrit jamais un fichier hôte `>=`, n'écrit rien hors `projects/`.
///
/// **Best-effort** : tolère tout échec `read_dir`/`copy` sans jamais faire échouer
/// l'appelant (la transition terminale du Run ne doit pas dépendre de ce merge).
/// `projects/` staging absent (run sans session) = no-op propre.
pub fn merge_back(home_root: &Path, sandbox_root: &Path, run_id: &str) -> Result<()> {
    for (_, set) in pdo_harness::probes::staging_sets() {
        for rel in set.transcripts {
            let src = staged_path(sandbox_root, run_id, rel);
            let dest = home_root.join(rel);
            if !src.is_dir() {
                continue; // rien écrit
            }
            let Ok(entries) = std::fs::read_dir(&src) else {
                continue;
            };
            // Un Run = plusieurs dirs encodés (un par worktree de node, manager,
            // merge-resolver). Itérer TOUS les sous-dossiers, jamais supposer un seul.
            for entry in entries.flatten() {
                let proj = entry.path();
                if proj.is_dir() {
                    copy_jsonl_tree(&proj, &dest.join(entry.file_name()));
                }
            }
        }
    }
    Ok(())
}

/// Supprime `<sandbox_root>/<run_id>/`. No-op si absent (best-effort, miroir de
/// `worktrees::reap_orphan_sub_worktree` du contexte workspace).
pub fn teardown(sandbox_root: &Path, run_id: &str) -> Result<()> {
    let _ = std::fs::remove_dir_all(staging_dir_for_run(sandbox_root, run_id));
    Ok(())
}

/// Stage 3 (ADR-0080): the adapter against a real temporary home.
#[cfg(test)]
mod tests {
    use super::set::{BYPASS_PERMISSIONS_KEY, SETTINGS_FILE};
    use super::*;

    /// Baseline de managed settings poussée par l'organisation, cachée par Claude Code
    /// dans `~/.claude/`. Depuis ADR-0063 le chemin runtime est une **entrée du staging set
    /// de `claude`** ([`pdo_harness::probes::CLAUDE_STAGING_SET`]) ; le nom court ne sert plus
    /// qu'à ces fixtures, qui le pinnent contre le set.
    const REMOTE_SETTINGS_FILE: &str = "remote-settings.json";
    /// The pre-#708 shape of `prepare`, for the tests that pin `claude`'s five
    /// guarantees: stage the profile, then fill `claude`'s set as the first claude
    /// spawn of the Run would. New tests that care about the *moment* of the copy
    /// call `prepare` and `fill_staging_set` separately.
    fn prepare_and_fill(
        home_root: &Path,
        sandbox_root: &Path,
        entries: &[String],
        run_id: &str,
        trusted_root: Option<&Path>,
    ) -> Result<PathBuf> {
        // Faithful to `ensure_ready`: `prepare` runs ONCE (gated on the staging dir),
        // then `fill_staging_set` (marker-gated, idempotent) fills `claude`'s set as
        // the first claude spawn would. Calling `prepare` unconditionally twice would
        // re-copy the host `settings.json` over the merged one while the marker made
        // the fill skip the bypass merge — a shape production never reaches.
        let staging = staging_dir_for_run(sandbox_root, run_id);
        if !staging.exists() {
            prepare(home_root, sandbox_root, entries, run_id)?;
        }
        fill_staging_set(
            home_root,
            sandbox_root,
            run_id,
            pdo_harness::registry::CLAUDE,
            trusted_root,
        )?;
        Ok(staging)
    }
    use std::os::unix::fs::PermissionsExt;

    // Un dirname encodé réaliste (cf. `pdo_harness::claude_transcript::encode_working_dir`) —
    // merge_back le copie VERBATIM (aucun ré-encodage, cf. bug #373).
    const ENC: &str = "-home-u--pdo-runs-X-worktree";
    const UUID: &str = "0f1e2d3c-aaaa-bbbb-cccc-ddddeeeeffff";

    fn mode_of(path: &Path) -> u32 {
        std::fs::symlink_metadata(path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn write_mode(path: &Path, content: &str, mode: u32) {
        write(path, content);
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn read_json(path: &Path) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    /// Contenu de la baseline org fabriquée par [`fabricate_home`]. Un *stand-in*
    /// : le vrai `~/.claude/remote-settings.json` porte un bearer OTEL de l'org et
    /// ne doit jamais apparaître dans un test, un log ou un artefact (#426).
    const ORG_BASELINE: &str = r#"{"org":"baseline"}"#;

    /// Construit un faux `~/.claude` réaliste sous `<home>/.claude` + le sibling
    /// `<home>/.claude.json`, couvrant l'allowlist, un symlink, un exécutable,
    /// des creds 0600, ET de l'état hôte volumineux qui doit rester EXCLU.
    /// La liste résolue du défaut `full` — le remplaçant mécanique de l'ancien
    /// `Mode::Full` dans ces tests. Passe par le vrai résolveur (et non par une copie
    /// de la constante) pour qu'un changement du défaut casse ces tests, pas les mente.
    fn full_entries() -> Vec<String> {
        let base = crate::domain::profile::base_entries(crate::domain::profile::FULL_PROFILE);
        crate::domain::profile::resolve_entry_list(&base, &[], &[]).entries
    }

    fn fabricate_home(home: &Path) {
        let claude = home.join(".claude");
        // Allowlist dirs.
        write(&claude.join("skills/foo/skill.md"), "# skill\n");
        write_mode(
            &claude.join("skills/foo/run.sh"),
            "#!/bin/sh\necho hi\n",
            0o755,
        );
        // Symlink relatif à l'intérieur de skills/foo → skill.md (INTRA-arbre).
        std::os::unix::fs::symlink("skill.md", claude.join("skills/foo/link.md")).unwrap();
        // Symlink ÉCHAPPANT : ~/.claude/skills/esc → ~/.agents/skills/esc (hors
        // ~/.claude). `../../.agents/…` depuis ~/.claude/skills/ résout à <home>/.agents.
        write(
            &home.join(".agents/skills/esc/SKILL.md"),
            "# escaped skill\n",
        );
        std::os::unix::fs::symlink("../../.agents/skills/esc", claude.join("skills/esc")).unwrap();
        write(&claude.join("plugins/bar/plugin.json"), "{}\n");
        write(&claude.join("agents/a.md"), "agent\n");
        write(&claude.join("commands/c.md"), "cmd\n");
        write(&claude.join("output-styles/s.md"), "style\n");
        // Allowlist files.
        write(&claude.join("settings.json"), r#"{"hooks":{"Stop":[]}}"#);
        write(&claude.join("settings.local.json"), r#"{"local":true}"#);
        // Baseline de managed settings de l'org — hors allowlist, stagée par le
        // set de `claude` (G2) dans les DEUX modes. Miroir de la fixture couche 3
        // (`sandbox_tracer::fabricate_host_claude`) : les deux doivent dériver
        // ensemble.
        write(&claude.join(REMOTE_SETTINGS_FILE), ORG_BASELINE);
        write_mode(
            &claude.join(".credentials.json"),
            r#"{"token":"secret"}"#,
            0o600,
        );
        write(&claude.join("CLAUDE.md"), "# global\n");
        write(&claude.join("RTK.md"), "# rtk\n");
        // État hôte volumineux — DOIT rester exclu.
        write(&claude.join("history.jsonl"), "{\"cmd\":\"ls\"}\n");
        write(&claude.join("file-history/big.bin"), "xxxxxxxxxx");
        write(&claude.join("session-env/env-1/data"), "junk");
        // Transcripts hôte pré-existants — NE doivent PAS être copiés par prepare.
        write(
            &claude.join(format!("projects/{ENC}/old.jsonl")),
            "{\"host\":1}\n",
        );
        // Sibling `.claude.json`.
        write(
            &home.join(".claude.json"),
            r#"{"host":"profile","oauthAccount":{"x":1}}"#,
        );
        // #432 : fichiers hôte HORS `~/.claude` qu'un profil peut déclarer en extra —
        // `.gitconfig` (fichier) et `.config/gh` (répertoire multi-segment). Présents même
        // quand le défaut `full` ne les porte pas : c'est ce qui rend testable
        // « le défaut ne stage rien hors `.claude` ». Miroir des fixtures couche 3
        // (`sandbox_tracer::fabricate_host_claude`, `sandbox_profiles::fabricate_host_home`) :
        // les trois doivent dériver ensemble.
        write(&home.join(".gitconfig"), HOST_GITCONFIG);
        write(
            &home.join(".config/gh/hosts.yml"),
            "github.com:\n  user: me\n",
        );
    }

    /// Contenu du `~/.gitconfig` fabriqué (comparé octet pour octet par les tests
    /// « l'hôte n'est jamais muté »).
    const HOST_GITCONFIG: &str = "[user]\n\tname = Host User\n\temail = host@example.com\n";

    #[test]
    fn prepare_full_reproduces_allowlist_and_excludes_projects() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        let staging = prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();
        assert_eq!(staging, staging_dir_for_run(sandbox_dir.path(), "run1"));
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Allowlist dirs présents.
        assert!(home.join("skills/foo/skill.md").is_file());
        assert!(home.join("plugins/bar/plugin.json").is_file());
        assert!(home.join("agents/a.md").is_file());
        assert!(home.join("commands/c.md").is_file());
        assert!(home.join("output-styles/s.md").is_file());
        // Allowlist files présents (dont hooks-via-settings).
        let settings = std::fs::read_to_string(home.join("settings.json")).unwrap();
        assert!(
            settings.contains("hooks"),
            "hooks vivent dans settings.json"
        );
        assert!(home.join("settings.local.json").is_file());
        assert!(home.join(".credentials.json").is_file());
        assert!(home.join("CLAUDE.md").is_file());
        assert!(
            home.join("RTK.md").is_file(),
            "*.md siblings captés par glob"
        );

        // `.claude.json` sibling copié depuis l'hôte (hors claude-home/), puis
        // repris par G4 : les clés hôte survivent, l'onboarding est ajouté. Le
        // fichier n'est PLUS byte-identique à l'hôte depuis #426 (G4 est
        // inconditionnelle) — mais rien de l'hôte n'est perdu.
        let staged_json = staged_claude_json(sandbox_dir.path(), "run1");
        assert!(staged_json.is_file());
        let json = read_json(&staged_json);
        assert_eq!(json["host"], serde_json::json!("profile"));
        assert_eq!(json["oauthAccount"]["x"], serde_json::json!(1));
        assert_eq!(json["hasCompletedOnboarding"], serde_json::json!(true));
        assert!(
            json.get("projects").is_none(),
            "trusted_root None → aucun bloc projects ajouté"
        );
        assert!(
            !home.join(".claude.json").exists(),
            ".claude.json ne doit PAS vivre dans claude-home/"
        );

        // `projects/` créé VIDE — transcripts hôte JAMAIS copiés.
        assert!(home.join("projects").is_dir());
        assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);
        assert!(!home.join(format!("projects/{ENC}/old.jsonl")).exists());

        // État hôte volumineux EXCLU.
        assert!(!home.join("history.jsonl").exists());
        assert!(!home.join("file-history").exists());
        assert!(!home.join("session-env").exists());
    }

    #[test]
    fn prepare_full_preserves_symlinks_and_exec_bit() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Symlink recréé COMME lien, cible verbatim.
        let link = home.join("skills/foo/link.md");
        let md = std::fs::symlink_metadata(&link).unwrap();
        assert!(
            md.file_type().is_symlink(),
            "le lien doit rester un symlink"
        );
        assert_eq!(
            std::fs::read_link(&link).unwrap(),
            PathBuf::from("skill.md")
        );

        // Exec bit conservé.
        assert_eq!(mode_of(&home.join("skills/foo/run.sh")), 0o755);
        // 0600 des creds préservé.
        assert_eq!(mode_of(&home.join(".credentials.json")), 0o600);
    }

    #[test]
    fn prepare_full_ignores_missing_entries() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        // Home minimal : uniquement settings.json, aucun autre membre de l'allowlist.
        write(&home_dir.path().join(".claude/settings.json"), "{}");

        // Ne doit pas paniquer / échouer sur les entrées absentes.
        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(home.join("settings.json").is_file());
        assert!(!home.join("skills").exists());
        assert!(!home.join(".credentials.json").exists());
        assert!(home.join("projects").is_dir());
        // G3 : le `settings.json` hôte était `{}` → la clé de bypass y est ajoutée.
        assert_eq!(
            read_json(&home.join(SETTINGS_FILE)),
            serde_json::json!({ BYPASS_PERMISSIONS_KEY: true })
        );
        // G4 est INCONDITIONNELLE depuis #426 : sans elle, l'hôte n'ayant pas de
        // `~/.claude.json`, rien n'était stagé — et `sandbox_container` monte ce
        // chemin toujours, donc Docker créait un *répertoire* par-dessus
        // `$HOME/.claude.json`. Le fixup G4 ferme ce footgun.
        let staged_json = staged_claude_json(sandbox_dir.path(), "run1");
        assert_eq!(
            read_json(&staged_json),
            serde_json::json!({ "hasCompletedOnboarding": true })
        );
        assert_eq!(mode_of(&staged_json), 0o600);
    }

    #[test]
    fn prepare_full_dereferences_escaping_symlink() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Le skill lié à ~/.agents (hors ~/.claude) est DÉRÉFÉRENCÉ : son contenu
        // atterrit comme fichier régulier, jamais un symlink dangling (sa cible
        // n'étant ni copiée ni montée, un lien verbatim disparaîtrait du conteneur).
        let esc = home.join("skills/esc/SKILL.md");
        let esc_md = std::fs::symlink_metadata(&esc).unwrap();
        assert!(
            esc_md.file_type().is_file(),
            "le skill échappant doit être déréférencé en fichier régulier"
        );
        assert_eq!(std::fs::read_to_string(&esc).unwrap(), "# escaped skill\n");
        assert!(
            !std::fs::symlink_metadata(home.join("skills/esc"))
                .unwrap()
                .file_type()
                .is_symlink(),
            "le dossier déréférencé ne doit pas être un lien"
        );

        // Non-régression : le lien INTRA-arbre reste un symlink (cible verbatim).
        let intra = home.join("skills/foo/link.md");
        assert!(
            std::fs::symlink_metadata(&intra)
                .unwrap()
                .file_type()
                .is_symlink(),
            "le lien intra-arbre doit rester un symlink"
        );
        assert_eq!(
            std::fs::read_link(&intra).unwrap(),
            PathBuf::from("skill.md")
        );
    }

    #[test]
    fn prepare_full_is_best_effort_on_unreadable_entry() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let claude = home_dir.path().join(".claude");
        // Un bon fichier + un symlink CASSÉ (cible inexistante) dans le même dir.
        write(&claude.join("skills/good/skill.md"), "# good\n");
        std::os::unix::fs::symlink("/nonexistent/pdo-broken-xyz", claude.join("skills/broken"))
            .unwrap();

        // Ne panique pas / n'échoue pas malgré l'entrée cassée (D3).
        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Le bon fichier est copié ; l'entrée cassée est sautée (rien de staged).
        assert!(home.join("skills/good/skill.md").is_file());
        assert!(
            std::fs::symlink_metadata(home.join("skills/broken")).is_err(),
            "le symlink cassé doit être sauté, pas recréé"
        );
    }

    #[test]
    fn prepare_full_seeds_trust_for_repo_root() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path()); // `.claude.json` hôte porte `oauthAccount`
        let trusted = Path::new("/repo/root");

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            Some(trusted),
        )
        .unwrap();

        let staged = staged_claude_json(sandbox_dir.path(), "run1");
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&staged).unwrap()).unwrap();
        // Merge NON-destructif : les clés hôte survivent.
        assert_eq!(json["host"], serde_json::json!("profile"));
        assert_eq!(json["oauthAccount"]["x"], serde_json::json!(1));
        // Confiance seedée pour le repo du Run.
        let entry = &json["projects"]["/repo/root"];
        assert_eq!(entry["hasTrustDialogAccepted"], serde_json::json!(true));
        assert_eq!(
            entry["hasCompletedProjectOnboarding"],
            serde_json::json!(true)
        );
        // 0600 réappliqué (le fichier peut porter un token).
        assert_eq!(mode_of(&staged), 0o600);
    }

    /// *L'*assertion que `minimal` == le staging set, ni plus ni moins (#426) : le
    /// contenu exact de `claude-home/` est le set de `claude`, alors que l'hôte porte tout
    /// l'appareil `full` (skills, plugins, settings riches…).
    #[test]
    fn prepare_minimal_stages_only_the_staging_set() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path()); // skills/settings existent → prouvent l'exclusion

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Ensemble EXACT (trié ASCII) = les fichiers du set, rien d'autre.
        let mut names: Vec<String> = std::fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                ".credentials.json".to_string(),
                "projects".to_string(),
                REMOTE_SETTINGS_FILE.to_string(),
                SETTINGS_FILE.to_string(),
            ]
        );
        assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);
        assert!(!home.join("skills").exists());
        assert_eq!(mode_of(&home.join(".credentials.json")), 0o600);
        // G3 : `settings.json` est la SYNTHÈSE à une clé, pas celui de l'hôte (qui
        // porte des `hooks`) — c'est la fourche synthèse-vs-copie du set.
        assert_eq!(
            read_json(&home.join(SETTINGS_FILE)),
            serde_json::json!({ BYPASS_PERMISSIONS_KEY: true })
        );

        // `.claude.json` minimal : onboarding seul, pas de bloc projects.
        let staged_json = staged_claude_json(sandbox_dir.path(), "run1");
        let json = read_json(&staged_json);
        assert_eq!(json["hasCompletedOnboarding"], serde_json::json!(true));
        assert!(json.get("projects").is_none(), "None → objet nu");
        assert_eq!(mode_of(&staged_json), 0o600);
    }

    #[test]
    fn prepare_minimal_seeds_trust_when_root_given() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        write_mode(
            &home_dir.path().join(".claude/.credentials.json"),
            "{}",
            0o600,
        );
        let trusted = Path::new("/repo/root");

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &[],
            "run1",
            Some(trusted),
        )
        .unwrap();

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(staged_claude_json(sandbox_dir.path(), "run1")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["hasCompletedOnboarding"], serde_json::json!(true));
        let entry = &json["projects"]["/repo/root"];
        assert_eq!(entry["hasTrustDialogAccepted"], serde_json::json!(true));
        assert_eq!(
            entry["hasCompletedProjectOnboarding"],
            serde_json::json!(true)
        );
    }

    #[test]
    fn prepare_minimal_bare_object_when_no_root() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(staged_claude_json(sandbox_dir.path(), "run1")).unwrap(),
        )
        .unwrap();
        // Exactement une clé : hasCompletedOnboarding.
        assert_eq!(json, serde_json::json!({ "hasCompletedOnboarding": true }));
    }

    /// Home fabriqué à la main SANS baseline org — le cas majoritaire (install sans
    /// organisation). Porte des credentials pour que G1 ait quelque chose à faire.
    fn fabricate_home_without_org(home: &Path) {
        write_mode(&home.join(".claude/.credentials.json"), "{}", 0o600);
    }

    #[test]
    fn prepare_full_stages_remote_settings() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        let staged = staged_claude_home(sandbox_dir.path(), "run1").join(REMOTE_SETTINGS_FILE);
        // Copie VERBATIM : la baseline est comparée au contenu côté Claude Code, une
        // ré-sérialisation ne serait pas forcément reconnue.
        assert_eq!(std::fs::read_to_string(&staged).unwrap(), ORG_BASELINE);
    }

    #[test]
    fn prepare_minimal_stages_remote_settings() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert_eq!(
            std::fs::read_to_string(home.join(REMOTE_SETTINGS_FILE)).unwrap(),
            ORG_BASELINE
        );
        // Preuve que c'est le PLANCHER et non l'allowlist `full` : rien du profil.
        assert!(!home.join("skills").exists());
    }

    #[test]
    fn prepare_full_without_remote_settings_is_a_logged_noop() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home_without_org(home_dir.path());

        // Aucune erreur : l'absence est le cas majoritaire (`info!` + no-op).
        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!home.join(REMOTE_SETTINGS_FILE).exists());
        // …et le set n'a PAS avorté en cours de route.
        assert!(home.join(SETTINGS_FILE).is_file());
        assert!(home.join("projects").is_dir());
    }

    #[test]
    fn prepare_minimal_without_remote_settings_is_a_logged_noop() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home_without_org(home_dir.path());

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!home.join(REMOTE_SETTINGS_FILE).exists());
        assert!(home.join(SETTINGS_FILE).is_file());
        assert!(home.join("projects").is_dir());
        // G1 tenue malgré le no-op de G2.
        assert_eq!(mode_of(&home.join(".credentials.json")), 0o600);
    }

    fn staged_settings(sandbox: &Path, run_id: &str) -> PathBuf {
        staged_claude_home(sandbox, run_id).join(SETTINGS_FILE)
    }

    #[test]
    fn prepare_full_merges_bypass_into_host_settings() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let host_settings = home_dir.path().join(".claude").join(SETTINGS_FILE);
        write(&host_settings, r#"{"hooks":{"Stop":[]},"model":"opus"}"#);

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        let staged = staged_settings(sandbox_dir.path(), "run1");
        let json = read_json(&staged);
        assert_eq!(json[BYPASS_PERMISSIONS_KEY], serde_json::json!(true));
        // Merge NON destructif : les clés hôte survivent.
        assert_eq!(json["model"], serde_json::json!("opus"));
        assert!(json["hooks"]["Stop"].is_array());
        // Pas de chmod, pas d'élargissement : le mode hôte est conservé (`fs::write`
        // tronque en place). Comparé au mode hôte, donc indépendant de l'umask.
        assert_eq!(mode_of(&staged), mode_of(&host_settings));
    }

    #[test]
    fn prepare_full_keeps_bypass_already_set_by_host() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        write(
            &home_dir.path().join(".claude").join(SETTINGS_FILE),
            &format!(r#"{{"{BYPASS_PERMISSIONS_KEY}":true,"model":"opus"}}"#),
        );

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        let json = read_json(&staged_settings(sandbox_dir.path(), "run1"));
        assert_eq!(json[BYPASS_PERMISSIONS_KEY], serde_json::json!(true));
        // Aucun artefact de duplication : toujours 2 clés.
        assert_eq!(json.as_object().unwrap().len(), 2);
    }

    #[test]
    fn prepare_full_overrides_host_bypass_false() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        write(
            &home_dir.path().join(".claude").join(SETTINGS_FILE),
            &format!(r#"{{"{BYPASS_PERMISSIONS_KEY}":false}}"#),
        );

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        // C'est une GARANTIE, pas un défaut : `insert`, jamais `entry().or_insert()`
        // (contrairement à `hasCompletedOnboarding` juste à côté). Un `false` hôte
        // bloquerait l'agent sur le prompt de bypass.
        let json = read_json(&staged_settings(sandbox_dir.path(), "run1"));
        assert_eq!(json[BYPASS_PERMISSIONS_KEY], serde_json::json!(true));
    }

    #[test]
    fn prepare_full_synthesises_settings_when_host_has_none() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home_without_org(home_dir.path()); // aucun settings.json hôte

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        assert_eq!(
            read_json(&staged_settings(sandbox_dir.path(), "run1")),
            serde_json::json!({ BYPASS_PERMISSIONS_KEY: true })
        );
    }

    #[test]
    fn prepare_minimal_synthesises_settings_ignoring_host() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path()); // settings hôte riches (hooks)

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        // La phrase d'ADR-0031 §1 sous forme de test : la même garantie, satisfaite
        // par une synthèse ici et par un merge en `full`.
        assert_eq!(
            read_json(&staged_settings(sandbox_dir.path(), "run1")),
            serde_json::json!({ BYPASS_PERMISSIONS_KEY: true })
        );
    }

    #[test]
    fn prepare_full_replaces_corrupt_host_settings() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        write(
            &home_dir.path().join(".claude").join(SETTINGS_FILE),
            "{ not json,",
        );

        // Dégradation GRACIEUSE (miroir du seed de trust) : en dur, un seul
        // caractère malformé dans `~/.claude/settings.json` empêcherait TOUT Run
        // sandboxé de démarrer.
        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        assert_eq!(
            read_json(&staged_settings(sandbox_dir.path(), "run1")),
            serde_json::json!({ BYPASS_PERMISSIONS_KEY: true })
        );
    }

    #[test]
    fn prepare_full_replaces_non_object_host_settings() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        // JSON valide, mais pas un objet → chemin de code distinct (garde `is_object`).
        write(
            &home_dir.path().join(".claude").join(SETTINGS_FILE),
            "[1,2]",
        );

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        assert_eq!(
            read_json(&staged_settings(sandbox_dir.path(), "run1")),
            serde_json::json!({ BYPASS_PERMISSIONS_KEY: true })
        );
    }

    /// La forme exécutable d'ADR-0031 §1, et le test qu'un relecteur lit pour
    /// répondre « le set est-il tenu ? ». La slice « profils » l'étendra
    /// (même corps, plus « … même avec l'entrée décochée »).
    #[test]
    fn prepare_staging_set_holds_in_both_modes() {
        // #432: "both modes" is now "the two virtual defaults' resolved lists", plus a
        // THIRD case that only profiles make reachable — `full` with the class-(b) entry
        // `.claude/settings.json` UNCHECKED. That case is the whole reason ADR-0031 §1
        // states the set as guarantees rather than files: G3 must still hold, by
        // synthesis, on a profile that explicitly refused the host file.
        let full_no_settings: Vec<String> = full_entries()
            .into_iter()
            .filter(|e| e != ".claude/settings.json")
            .collect();
        for (mode, entries) in [
            ("full", full_entries()),
            ("minimal", Vec::new()),
            ("full-without-settings", full_no_settings),
        ] {
            let entries: &[String] = &entries;
            let home_dir = tempfile::tempdir().unwrap();
            let sandbox_dir = tempfile::tempdir().unwrap();
            fabricate_home(home_dir.path());
            let trusted = Path::new("/repo/root");

            prepare_and_fill(
                home_dir.path(),
                sandbox_dir.path(),
                entries,
                "run1",
                Some(trusted),
            )
            .unwrap();
            let home = staged_claude_home(sandbox_dir.path(), "run1");

            // G1 — credentials valides, 0600 préservé.
            assert_eq!(
                mode_of(&home.join(".credentials.json")),
                0o600,
                "G1 en {mode:?}"
            );
            // G2 — baseline de managed settings de l'org.
            assert_eq!(
                std::fs::read_to_string(home.join(REMOTE_SETTINGS_FILE)).unwrap(),
                ORG_BASELINE,
                "G2 en {mode:?}"
            );
            // G3 — bypass permissions accepté.
            assert_eq!(
                read_json(&home.join(SETTINGS_FILE))[BYPASS_PERMISSIONS_KEY],
                serde_json::json!(true),
                "G3 en {mode:?}"
            );
            // G4 — confiance pré-accordée à la racine du Run + onboarding.
            let json = read_json(&staged_claude_json(sandbox_dir.path(), "run1"));
            assert_eq!(
                json["projects"]["/repo/root"]["hasTrustDialogAccepted"],
                serde_json::json!(true),
                "G4 (trust) en {mode:?}"
            );
            assert_eq!(
                json["hasCompletedOnboarding"],
                serde_json::json!(true),
                "G4 (onboarding) en {mode:?}"
            );
            // G5 — puits de transcripts créé VIDE.
            let projects = home.join("projects");
            assert!(projects.is_dir(), "G5 en {mode:?}");
            assert_eq!(
                std::fs::read_dir(&projects).unwrap().count(),
                0,
                "G5 vide en {mode:?}"
            );
        }
    }

    /// Fige ADR-0031 §6 (« `prepare` est additif : il copie ou écrase, il ne
    /// supprime jamais ») et répond à la question du double write sur
    /// `settings.json` en `full` : le second merge n'écrase ni ne duplique rien.
    #[test]
    fn prepare_twice_keeps_the_staging_set_and_is_additive() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());
        let trusted = Path::new("/repo/root");
        let (home_p, sandbox_p) = (home_dir.path(), sandbox_dir.path());

        prepare_and_fill(home_p, sandbox_p, &full_entries(), "run1", Some(trusted)).unwrap();
        // Le conteneur (simulé) a écrit un transcript dans le puits.
        stage_transcript(sandbox_p, "run1", &format!("{ENC}/s.jsonl"), "line\n");

        prepare_and_fill(home_p, sandbox_p, &full_entries(), "run1", Some(trusted)).unwrap();

        let home = staged_claude_home(sandbox_p, "run1");
        let settings = read_json(&home.join(SETTINGS_FILE));
        assert_eq!(settings[BYPASS_PERMISSIONS_KEY], serde_json::json!(true));
        assert!(
            settings["hooks"]["Stop"].is_array(),
            "le 2e merge n'a pas écrasé les clés hôte"
        );
        assert_eq!(
            settings.as_object().unwrap().len(),
            2,
            "ni dupliqué la clé de bypass"
        );
        assert_eq!(
            std::fs::read_to_string(home.join(REMOTE_SETTINGS_FILE)).unwrap(),
            ORG_BASELINE
        );
        assert_eq!(
            read_json(&staged_claude_json(sandbox_p, "run1"))["projects"]["/repo/root"]
                ["hasTrustDialogAccepted"],
            serde_json::json!(true)
        );
        // ADDITIF : le transcript écrit entre les deux `prepare` survit.
        assert_eq!(
            std::fs::read_to_string(home.join(format!("projects/{ENC}/s.jsonl"))).unwrap(),
            "line\n"
        );
    }

    /// ADR-0031 §4, la moitié *copie* : une entrée hors `.claude` atterrit sous
    /// `<staging>/home/<rel>`, jamais ailleurs — et surtout jamais dans `claude-home/`,
    /// où elle finirait à `$HOME/.claude/<rel>` côté conteneur.
    #[test]
    fn prepare_copies_a_home_extra_under_staging_home() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        let entries = vec![".gitconfig".to_string(), ".config/gh".to_string()];
        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &entries, "run1", None).unwrap();

        let extras = staged_home_extras(sandbox_dir.path(), "run1");
        assert_eq!(
            std::fs::read_to_string(extras.join(".gitconfig")).unwrap(),
            HOST_GITCONFIG,
            "un fichier extra est copié verbatim"
        );
        assert!(
            extras.join(".config/gh/hosts.yml").is_file(),
            "un répertoire extra multi-segment est copié en profondeur"
        );
        // Rien n'a fui dans le home stagé (le mount `.claude` ne doit PAS les porter).
        let claude_home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!claude_home.join(".gitconfig").exists());
        assert!(!claude_home.join(".config").exists());
        // Et l'hôte n'a pas été touché.
        assert_eq!(
            std::fs::read_to_string(home_dir.path().join(".gitconfig")).unwrap(),
            HOST_GITCONFIG
        );
    }

    /// La copie d'une entrée extra utilise `copy_root = canonicalize(l'entrée)`, pas
    /// `~/.claude`. Avec `~/.claude` comme `copy_root`, tout symlink INTERNE à l'entrée
    /// serait classé « échappant » et déréférencé — piège explicite du plan #432 D5.
    #[test]
    fn a_home_extras_internal_symlink_stays_a_symlink() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());
        let gh = home_dir.path().join(".config/gh");
        std::os::unix::fs::symlink("hosts.yml", gh.join("alias.yml")).unwrap();

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &[".config/gh".to_string()],
            "run1",
            None,
        )
        .unwrap();

        let staged = staged_home_extras(sandbox_dir.path(), "run1").join(".config/gh/alias.yml");
        assert!(
            std::fs::symlink_metadata(&staged)
                .unwrap()
                .file_type()
                .is_symlink(),
            "un lien intra-entrée doit rester un lien, pas être déréférencé"
        );
    }

    /// Manquant sur l'hôte = `warn!` + skip, pour un extra COMME pour une entrée du
    /// défaut. L'échec dur est écarté (plan D5) : il ferait dépendre la politique de qui a
    /// tapé le chemin, et sur une instance à Triggers horaires désinstaller `gh` tuerait
    /// chaque tir. La règle M1 ci-dessous supprime le danger réel.
    #[test]
    fn prepare_skips_a_home_extra_absent_on_the_host() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &[".not-here".to_string(), ".gitconfig".to_string()],
            "run1",
            None,
        )
        .expect("une entrée absente ne fait pas échouer le prep");

        let extras = staged_home_extras(sandbox_dir.path(), "run1");
        assert!(!extras.join(".not-here").exists());
        assert!(
            extras.join(".gitconfig").is_file(),
            "les autres passent quand même"
        );
    }

    /// ADR-0031 §4, la moitié *mount* — et la dédup, qui n'est PAS un cas spécial : les
    /// entrées sous `.claude/` et `.claude.json` sont déjà servies par les mounts fixes,
    /// donc `landing()` les exclut par construction.
    #[test]
    fn extra_mounts_only_covers_home_exceptions() {
        let sandbox_dir = tempfile::tempdir().unwrap();
        let host_home = Path::new("/home/u");
        let extras_root = staged_home_extras(sandbox_dir.path(), "run1");
        std::fs::create_dir_all(extras_root.join(".config/gh")).unwrap();
        std::fs::write(extras_root.join(".gitconfig"), "x").unwrap();

        let entries: Vec<String> = [
            ".claude.json",
            ".claude/skills",
            ".claude/*.md",
            ".gitconfig",
            ".config/gh",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let mounts = extra_mounts(sandbox_dir.path(), "run1", host_home, &entries);
        assert_eq!(
            mounts,
            vec![
                StagedMount {
                    source: extras_root.join(".config/gh"),
                    target: host_home.join(".config/gh"),
                },
                StagedMount {
                    source: extras_root.join(".gitconfig"),
                    target: host_home.join(".gitconfig"),
                },
            ],
            "seules les exceptions `$HOME`, triées par chemin relatif"
        );
    }

    /// **Règle M1**, non négociable (sondée en Docker réel 29.2.1) : un `-v` dont la
    /// source hôte n'existe pas fait créer par Docker un répertoire `root:root 0755`.
    /// Trois dégâts en escalade — un mount de *fichier* manquant devient un *répertoire*
    /// par-dessus `$HOME/.gitconfig` (git plante en lecture) ; un mount de *répertoire*
    /// devient inscriptible par personne (uid 1000 dedans) ; et pour un chemin
    /// multi-segment (`.config/gh`) Docker crée `<staging>/home/.config` en root aussi,
    /// ce qui fait échouer le `remove_dir_all` de `teardown` en EACCES — erreur avalée par
    /// son `let _ =` — laissant un staging de ~1 Go définitivement indélébile par le
    /// daemon. Ça alimente la récurrence disque connue, silencieusement.
    #[test]
    fn extra_mounts_never_mounts_a_source_that_does_not_exist() {
        let sandbox_dir = tempfile::tempdir().unwrap();
        // Rien n'a été stagé (l'entrée était absente de l'hôte) → aucun mount.
        let mounts = extra_mounts(
            sandbox_dir.path(),
            "run1",
            Path::new("/home/u"),
            &[".gitconfig".to_string()],
        );
        assert!(mounts.is_empty(), "M1 : pas de source, pas de `-v`");
    }

    /// `extra_mounts` dérive de `liste gelée × disque`, PAS d'une valeur de retour de
    /// `prepare` — `ensure_ready` saute `prepare` quand le staging existe déjà (3 de ses
    /// 4 appelants). La dérivation doit donc être **totale** : même réponse que `prepare`
    /// ait tourné ou non.
    #[test]
    fn extra_mounts_is_total_whether_prepare_ran_or_not() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());
        let host_home = Path::new("/home/u");
        let entries = vec![".gitconfig".to_string()];

        // Avant `prepare` : rien sur disque → aucun mount (M1).
        assert!(extra_mounts(sandbox_dir.path(), "run1", host_home, &entries).is_empty());

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &entries, "run1", None).unwrap();

        // Après : la même liste gelée donne le mount, sans que `prepare` l'ait renvoyé.
        let after = extra_mounts(sandbox_dir.path(), "run1", host_home, &entries);
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].target, host_home.join(".gitconfig"));
        // Idempotent : un second appel (le cas « staging déjà présent ») est identique.
        assert_eq!(
            extra_mounts(sandbox_dir.path(), "run1", host_home, &entries),
            after
        );
    }

    /// Le glob du défaut reste un glob à UN niveau : il capte les `*.md` top-level de
    /// `~/.claude` et rien de plus profond.
    #[test]
    fn the_md_glob_matches_one_level_only() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());
        write(
            &home_dir.path().join(".claude/deep/nested.md"),
            "# nested\n",
        );

        prepare_and_fill(
            home_dir.path(),
            sandbox_dir.path(),
            &[".claude/*.md".to_string()],
            "run1",
            None,
        )
        .unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(home.join("CLAUDE.md").is_file());
        assert!(home.join("RTK.md").is_file());
        assert!(!home.join("deep").exists(), "un niveau, pas de récursion");
        // Et rien d'autre du défaut n'a été stagé : la liste ne portait qu'une entrée.
        assert!(!home.join("skills").exists());
    }

    /// Une liste VIDE est le no-op exact de l'ancien bras `Mode::Minimal => {}` : rien en
    /// propre, tout le set.
    #[test]
    fn an_empty_entry_list_stages_the_staging_set_and_nothing_else() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare_and_fill(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!home.join("skills").exists());
        assert!(!home.join("CLAUDE.md").exists());
        // No PROFILE extra was staged; the only thing under `home/` is the empty
        // `.pi/agent` mount anchor (#708) — created empty, never a profile copy.
        let extras = staged_home_extras(sandbox_dir.path(), "run1");
        let pi_anchor = extras.join(".pi/agent");
        assert!(pi_anchor.is_dir() && std::fs::read_dir(&pi_anchor).unwrap().next().is_none());
        assert!(!extras.join(".gitconfig").exists() && !extras.join(".config").exists());
        // Plancher intact.
        assert!(home.join(".credentials.json").is_file());
        assert!(home.join("projects").is_dir());
        assert_eq!(
            read_json(&home.join(SETTINGS_FILE))[BYPASS_PERMISSIONS_KEY],
            serde_json::json!(true)
        );
    }

    /// Écrit un transcript de staging (projects/<ENC>/...).
    fn stage_transcript(sandbox: &Path, run_id: &str, rel: &str, content: &str) {
        let p = staged_claude_home(sandbox, run_id)
            .join("projects")
            .join(rel);
        write(&p, content);
    }

    fn host_projects(home: &Path) -> PathBuf {
        home.join(".claude/projects")
    }

    #[test]
    fn merge_back_copies_only_jsonl_recursively() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        // Session top-level + sous-agent imbriqué (profondeur), + non-jsonl à jeter.
        stage_transcript(sandbox, "run1", &format!("{ENC}/sess.jsonl"), "s\n");
        stage_transcript(
            sandbox,
            "run1",
            &format!("{ENC}/{UUID}/subagents/agent.jsonl"),
            "a\n",
        );
        stage_transcript(
            sandbox,
            "run1",
            &format!("{ENC}/notes.md"),
            "not a transcript",
        );
        stage_transcript(sandbox, "run1", &format!("{ENC}/.meta.json"), "{}");

        merge_back(home, sandbox, "run1").unwrap();

        let hp = host_projects(home);
        assert_eq!(
            std::fs::read_to_string(hp.join(format!("{ENC}/sess.jsonl"))).unwrap(),
            "s\n"
        );
        assert_eq!(
            std::fs::read_to_string(hp.join(format!("{ENC}/{UUID}/subagents/agent.jsonl")))
                .unwrap(),
            "a\n"
        );
        // Non-`.jsonl` jetés.
        assert!(!hp.join(format!("{ENC}/notes.md")).exists());
        assert!(!hp.join(format!("{ENC}/.meta.json")).exists());
    }

    #[test]
    fn merge_back_is_idempotent() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        stage_transcript(sandbox, "run1", &format!("{ENC}/sess.jsonl"), "line-1\n");
        merge_back(home, sandbox, "run1").unwrap();

        let host_file = host_projects(home).join(format!("{ENC}/sess.jsonl"));
        assert_eq!(std::fs::read_to_string(&host_file).unwrap(), "line-1\n");

        // Sentinelle de MÊME longueur côté hôte : si le 2e appel réécrivait le
        // fichier, la sentinelle serait écrasée. len(hôte)==len(staging) → no-op.
        std::fs::write(&host_file, "SENT-1\n").unwrap();
        assert_eq!("SENT-1\n".len(), "line-1\n".len());

        merge_back(home, sandbox, "run1").unwrap();
        assert_eq!(
            std::fs::read_to_string(&host_file).unwrap(),
            "SENT-1\n",
            "2e appel = no-op sur fichier de taille égale (jamais réécrit)"
        );
    }

    #[test]
    fn merge_back_re_merge_after_resume_grows() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());
        let rel = format!("{ENC}/sess.jsonl");

        // 1er merge : N lignes.
        stage_transcript(sandbox, "run1", &rel, "l1\nl2\n");
        merge_back(home, sandbox, "run1").unwrap();
        let host_file = host_projects(home).join(&rel);
        assert_eq!(std::fs::read_to_string(&host_file).unwrap(), "l1\nl2\n");

        // Resume (--continue) : le staging grossit → 2e merge capte la croissance.
        stage_transcript(sandbox, "run1", &rel, "l1\nl2\nl3\nl4\n");
        merge_back(home, sandbox, "run1").unwrap();
        assert_eq!(
            std::fs::read_to_string(&host_file).unwrap(),
            "l1\nl2\nl3\nl4\n"
        );

        // 3e appel sans changement → no-op (sentinelle de taille égale survit).
        std::fs::write(&host_file, "X1\nX2\nX3\nX4\n").unwrap();
        merge_back(home, sandbox, "run1").unwrap();
        assert_eq!(
            std::fs::read_to_string(&host_file).unwrap(),
            "X1\nX2\nX3\nX4\n"
        );
    }

    #[test]
    fn merge_back_never_clobbers_larger_host_file() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());
        let rel = format!("{ENC}/sess.jsonl");

        // Fichier hôte authentique, PLUS grand que le staging.
        let host_file = host_projects(home).join(&rel);
        write(&host_file, "host-line-1\nhost-line-2\nhost-line-3\n");
        stage_transcript(sandbox, "run1", &rel, "short\n");

        merge_back(home, sandbox, "run1").unwrap();
        assert_eq!(
            std::fs::read_to_string(&host_file).unwrap(),
            "host-line-1\nhost-line-2\nhost-line-3\n",
            "fichier hôte plus grand jamais écrasé"
        );
    }

    #[test]
    fn merge_back_creates_missing_host_dirs() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        // Aucun ~/.claude/projects préexistant.
        assert!(!host_projects(home).exists());
        stage_transcript(sandbox, "run1", &format!("{ENC}/sess.jsonl"), "s\n");

        merge_back(home, sandbox, "run1").unwrap();
        assert!(host_projects(home)
            .join(format!("{ENC}/sess.jsonl"))
            .is_file());
    }

    #[test]
    fn merge_back_noop_when_no_projects() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        // Staging minimal : claude-home existe mais sans projects/.
        std::fs::create_dir_all(staged_claude_home(sandbox, "run1")).unwrap();

        merge_back(home, sandbox, "run1").unwrap(); // Ok, aucune écriture hôte.
        assert!(!host_projects(home).exists());

        // Staging entièrement absent → également no-op propre.
        merge_back(home, sandbox, "absent-run").unwrap();
        assert!(!host_projects(home).exists());
    }

    #[test]
    fn merge_back_writes_nothing_outside_projects() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        // Config hôte pré-existante (sentinelles hors projects/).
        write(&home.join(".claude/settings.json"), "HOST-SETTINGS");
        write(&home.join(".claude/history.jsonl"), "HOST-HISTORY");

        // Le staging contient aussi une config parasite qui NE doit pas fuiter :
        // merge_back ne lit que projects/.
        stage_transcript(sandbox, "run1", &format!("{ENC}/sess.jsonl"), "s\n");
        write(
            &staged_claude_home(sandbox, "run1").join("settings.json"),
            "STAGING-SETTINGS",
        );

        merge_back(home, sandbox, "run1").unwrap();

        assert_eq!(
            std::fs::read_to_string(home.join(".claude/settings.json")).unwrap(),
            "HOST-SETTINGS"
        );
        assert_eq!(
            std::fs::read_to_string(home.join(".claude/history.jsonl")).unwrap(),
            "HOST-HISTORY"
        );
        assert!(host_projects(home)
            .join(format!("{ENC}/sess.jsonl"))
            .is_file());
    }

    #[test]
    fn teardown_purges_staging() {
        let sandbox_dir = tempfile::tempdir().unwrap();
        let sandbox = sandbox_dir.path();
        std::fs::create_dir_all(staged_claude_home(sandbox, "run1").join("projects")).unwrap();
        assert!(staging_dir_for_run(sandbox, "run1").exists());

        teardown(sandbox, "run1").unwrap();
        assert!(!staging_dir_for_run(sandbox, "run1").exists());
    }

    #[test]
    fn teardown_absent_is_ok() {
        let sandbox_dir = tempfile::tempdir().unwrap();
        teardown(sandbox_dir.path(), "never-created").unwrap();
    }

    #[test]
    fn prepare_minimal_then_merge_and_teardown_roundtrip() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        prepare_and_fill(home, sandbox, &[], "run1", None).unwrap();
        // Le conteneur (simulé) écrit un transcript dans le puits projects/.
        stage_transcript(sandbox, "run1", &format!("{ENC}/sess.jsonl"), "hello\n");

        merge_back(home, sandbox, "run1").unwrap();
        assert_eq!(
            std::fs::read_to_string(host_projects(home).join(format!("{ENC}/sess.jsonl"))).unwrap(),
            "hello\n"
        );

        teardown(sandbox, "run1").unwrap();
        assert!(!staging_dir_for_run(sandbox, "run1").exists());
        // Le transcript mergé côté hôte survit au teardown.
        assert!(host_projects(home)
            .join(format!("{ENC}/sess.jsonl"))
            .is_file());
    }

    /// #708: filling `pi`'s set copies `.pi/agent` (minus `sessions/`) into the staged
    /// pi home, creates the empty `sessions/` sink, and reports pi's two env vars — no
    /// fixup, no `.claude` write. A second fill is a marker no-op.
    #[test]
    fn filling_pi_set_stages_the_agent_home_and_reports_env() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".pi/agent/extensions")).unwrap();
        std::fs::write(home.join(".pi/agent/auth.json"), "{\"token\":1}").unwrap();
        std::fs::write(home.join(".pi/agent/models.json"), "[]").unwrap();
        // A host session file that must NOT be copied on the way in.
        std::fs::create_dir_all(home.join(".pi/agent/sessions/enc")).unwrap();
        std::fs::write(home.join(".pi/agent/sessions/enc/s.jsonl"), "x\n").unwrap();
        let sandbox_root = tmp.path().join("sandbox");

        let fill =
            fill_staging_set(&home, &sandbox_root, "r1", pdo_harness::registry::PI, None).unwrap();
        assert_eq!(
            fill.env(),
            vec![
                ("PI_SKIP_VERSION_CHECK".to_string(), "1".to_string()),
                ("PI_TELEMETRY".to_string(), "0".to_string()),
            ]
        );
        let staged = staged_path(&sandbox_root, "r1", ".pi/agent");
        assert!(staged.join("auth.json").is_file(), "auth copied");
        assert!(staged.join("models.json").is_file(), "catalogue copied");
        assert!(staged.join("extensions").is_dir(), "extensions copied");
        let sink = staged.join("sessions");
        assert!(sink.is_dir(), "sessions sink created");
        assert!(
            !sink.join("enc/s.jsonl").exists(),
            "host sessions are NOT copied in (transcripts sink is empty)"
        );

        // Idempotent: second fill is a marker no-op (AlreadyFilled), env still reported.
        let again =
            fill_staging_set(&home, &sandbox_root, "r1", pdo_harness::registry::PI, None).unwrap();
        assert!(!again.env().is_empty());
    }

    /// A claude-only Run never carries pi's auth: filling only `claude` leaves the
    /// staged pi home empty (it exists as a mount anchor, but nothing was copied).
    #[test]
    fn a_claude_only_run_has_no_pi_auth_in_its_staging() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        std::fs::write(home.join(".claude/.credentials.json"), "{}").unwrap();
        std::fs::create_dir_all(home.join(".pi/agent")).unwrap();
        std::fs::write(home.join(".pi/agent/auth.json"), "{}").unwrap();
        let sandbox_root = tmp.path().join("sandbox");

        prepare(&home, &sandbox_root, &[], "r1").unwrap();
        fill_staging_set(
            &home,
            &sandbox_root,
            "r1",
            pdo_harness::registry::CLAUDE,
            None,
        )
        .unwrap();

        let pi_home = staged_path(&sandbox_root, "r1", ".pi/agent");
        assert!(
            !pi_home.join("auth.json").exists(),
            "a claude-only Run must not have pi's auth in its staging"
        );
    }
}
