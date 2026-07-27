//! Pure per-run sandbox home-staging (#404, slice A du PRD #403).
//!
//! Zéro Docker. Miroir de [`crate::worktree_ops`] : pas d'`AppState`, pas
//! d'async, pas de lecture d'env dans le cœur — seulement `&Path` / `&str` in,
//! path-math ou `std::fs` out. `HOME` n'est lu QUE par le résolveur de bord
//! [`default_roots_from_env`].
//!
//! Ce module gère le cycle de vie du *staged Claude home* d'un Run sandboxé :
//! [`prepare`] (seeder), [`merge_back`] (récupérer les transcripts), [`teardown`]
//! (purger). Les slices sœurs le **consomment** mais ne sont **pas** ici :
//! - #406 monte `claude-home/` → `$HOME/.claude` et `.claude.json` → `$HOME/.claude.json` ;
//! - #407 câble `prepare`/`teardown` dans le run-advance (ADR-0030) ;
//! - #408 câble `merge_back` (transition terminale + `cleanup_run`) + pointe
//!   stale-detection/coût vers le staging (seam [`crate::sandbox_run::transcripts_root`]).
//!
//! ## Décisions de conception (voir la section « Sandbox » de `CONTEXT.md`)
//! - **Allowlist, jamais denylist.** Copier « tout `~/.claude` sauf `projects/` »
//!   embarquerait tout l'état hôte transitoire (`history.jsonl`, `session-env/`,
//!   `file-history/`…) — fuite d'isolation + fragile aux futures versions de Claude
//!   Code. On copie une liste explicite. Depuis #432 cette liste est **nommée et
//!   éditable** : c'est le *profil de staging* ([`crate::sandbox_profile`]), résolu et
//!   **gelé** dans `RunStarted`. Ce module ne connaît plus de « mode » — il reçoit une
//!   liste d'entrées, et la vide (`minimal`) est un no-op de phase 1.
//! - **Volume assumé ~1 Go/run** (#409, mesuré ; dominé par `plugins/*/node_modules`,
//!   requis par les serveurs MCP *dans* le conteneur — délibérément non strippés).
//!   Dette disque : le staging n'est purgé qu'au `cleanup_run` → à surveiller au
//!   regard de la récurrence disque connue (recette janitor pour l'usage massif).
//! - **Symlinks échappants déréférencés** (#409). Une part notable des skills sont
//!   des liens relatifs vers `~/.agents` (hors `~/.claude`, ni copié ni monté) :
//!   recréés verbatim ils dangleraient dans le conteneur (skills invisibles).
//!   [`copy_tree_preserving`] copie donc le CONTENU réel des liens qui **sortent** de
//!   l'arbre `~/.claude` ; les liens **intra-arbre** (cycles `node_modules/.bin`)
//!   restent des liens. Walk **best-effort par entrée** : un `~/.claude` volumineux
//!   que d'autres process Claude mutent ne doit jamais faire échouer le Run.
//! - **`merge_back` récurse.** ~42 % des transcripts vivent dans
//!   `projects/<enc>/<uuid>/subagents/*.jsonl` (profondeur 9). Le copy-set doit
//!   *égaler* le read-set de [`crate::run_cost`] (`collect_jsonl_recursive`),
//!   sinon le coût des runs sandboxés est sous-estimé (régression silencieuse).
//! - **Le plancher de staging est profil-agnostique** (#426, ADR-0031 §1).
//!   [`prepare`] est en **deux phases** : matérialisation des entrées du profil, puis
//!   [`enforce_staging_floor`] qui tient **cinq garanties** quel que soit le profil —
//!   chacune satisfaite soit par une copie de l'hôte, soit par une synthèse de repli.
//!   `minimal`, c'est exactement le plancher (liste vide). Les trois garanties qui
//!   désarment un dialogue bloquant (confiance, managed settings de l'org, bypass
//!   permissions) ne sont pas cosmétiques : un agent non surveillé se figerait dessus.
//! - **Une entrée hors `.claude` est copiée puis montée** (#432, ADR-0031 §4) :
//!   `<staging>/home/<rel>` → `$HOME/<rel>` en rw, **jamais** un bind direct du
//!   fichier hôte. Un `git config --global` du conteneur touche la copie, pas le
//!   `~/.gitconfig` de l'utilisateur. Une entrée **sous** `.claude/` ne reçoit aucun
//!   `-v` propre — conséquence du classificateur unique
//!   [`crate::sandbox_profile::landing`], partagé par [`prepare`] et [`extra_mounts`].

// #408: `merge_back` is now wired; the rest of the module (path-math + effects)
// is consumed by #406/#407.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use tracing::{info, warn};

/// Baseline de managed settings poussée par l'organisation, cachée par Claude Code
/// dans `~/.claude/`. Garantie G2 : recopiée dans les **deux** modes, jamais via
/// [`FULL_ALLOWLIST_FILES`]. Sans elle, la session bute sur le dialogue
/// « managed settings require approval » (le consentement est comparé au contenu
/// de ce fichier) et un Run autonome reste planté.
const REMOTE_SETTINGS_FILE: &str = "remote-settings.json";

/// Le `settings.json` du home stagé — porteur de la garantie G3.
const SETTINGS_FILE: &str = "settings.json";

/// Clé top-level qui désarme le prompt de confirmation du
/// `--dangerously-skip-permissions`. Garantie G3 : un agent non surveillé s'y
/// bloque. Résolue par un **OU monotone** sur les tiers de settings, donc un
/// `true` dans le `settings.json` stagé suffit — et un `false` hôte doit être
/// **écrasé** (d'où `insert` et non `entry().or_insert()`).
const BYPASS_PERMISSIONS_KEY: &str = "skipDangerousModePermissionPrompt";

// -- path math (pur, sans IO) ------------------------------------------------

/// `<sandbox_root>/<run_id>` — racine du staging d'un Run (les 2 sources de mount
/// + le `.claude.json`).
pub(crate) fn staging_dir_for_run(sandbox_root: &Path, run_id: &str) -> PathBuf {
    sandbox_root.join(run_id)
}

/// `<staging_dir>/claude-home` — le *staged Claude home* (→ `$HOME/.claude` côté
/// conteneur, monté tel quel par #406).
pub(crate) fn staged_claude_home(sandbox_root: &Path, run_id: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id).join("claude-home")
}

/// `<staging_dir>/.claude.json` — sibling de `claude-home/` (→ `$HOME/.claude.json`
/// côté conteneur, monté séparément par #406). **Caché + hors `claude-home/`** :
/// s'il vivait dans `claude-home/` il atterrirait à `$HOME/.claude/.claude.json`,
/// invisible pour Claude Code.
pub(crate) fn staged_claude_json(sandbox_root: &Path, run_id: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id).join(".claude.json")
}

/// `<staging_dir>/home` — racine des **entrées d'exception `$HOME`** (#432,
/// ADR-0031 §4). Une entrée hors `.claude` y est *copiée* (`<staging>/home/<rel>`)
/// puis bind-montée rw à `$HOME/<rel>` : **jamais** un bind direct du fichier hôte,
/// sinon un agent en `--dangerously-skip-permissions` qui bute sur
/// `unable to auto-detect email address` réécrirait le `~/.gitconfig` de
/// l'utilisateur.
pub(crate) fn staged_home_extras(sandbox_root: &Path, run_id: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id).join("home")
}

/// Un bind-mount d'exception `$HOME` : `source` (dans le staging) → `target` (dans
/// le conteneur). Calculé par [`extra_mounts`], consommé par
/// [`crate::sandbox_container::ContainerSpec::extra_mounts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedMount {
    pub(crate) source: PathBuf,
    pub(crate) target: PathBuf,
}

/// La queue variable de `-v` qu'une liste d'entrées gelée produit (#432).
///
/// **Pas** une valeur de retour de [`prepare`] : `ensure_ready` saute `prepare` quand
/// le staging existe déjà, ce qui est le cas de 3 de ses 4 appelants. Dériver de
/// `liste gelée × disque` est **total** — même réponse que `prepare` ait tourné ou
/// non.
///
/// Deux filtres, dans cet ordre :
/// 1. `landing()` == [`crate::sandbox_profile::Landing::HomeExtra`] — une entrée sous
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
pub(crate) fn extra_mounts(
    sandbox_root: &Path,
    run_id: &str,
    host_home: &Path,
    entries: &[String],
) -> Vec<StagedMount> {
    let extras_root = staged_home_extras(sandbox_root, run_id);
    let mut rels: Vec<&str> = entries
        .iter()
        .filter_map(|entry| match crate::sandbox_profile::landing(entry) {
            crate::sandbox_profile::Landing::HomeExtra { rel } => Some(rel),
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

// -- effets (sync std::fs, anyhow + .context) --------------------------------

/// Seede le *staged Claude home* et renvoie sa racine (`<sandbox_root>/<run_id>`).
///
/// **Deux phases** (#426, ADR-0031 §1) :
/// 1. *matérialisation du profil* — une passe sur la liste d'entrées **gelée** ;
/// 2. *[`enforce_staging_floor`]* — profil-agnostique, tient les **cinq garanties**
///    (credentials, managed settings de l'org, bypass permissions, confiance sur
///    `trusted_root` + onboarding, `projects/` vide).
///
/// La phase 2 tourne **après** la phase 1, jamais dedans : le plancher est un
/// *check-then-repair* sur ce qui est effectivement posé sur disque (« satisfaite
/// soit par une copie de l'hôte, soit par une synthèse de repli » n'est décidable
/// qu'à ce moment-là). Le double write sur `settings.json` quand l'entrée est cochée
/// (copiée par le profil, puis mergée par le plancher) est **voulu** : le plancher est
/// merge-aware, il ne doit pas être profil-aware.
///
/// `entries` (#432) occupe le slot de l'ancien `mode` : c'est la liste **résolue et
/// gelée** dans `RunStarted`, jamais le réglage vivant (ADR-0031 §6). `sandbox_staging::Mode`
/// a disparu — la phase 2 est déjà agnostique depuis #426 et les défauts virtuels sont
/// des listes nommées, `minimal` étant la vide. Garder un `Mode` à côté d'une liste
/// recréerait exactement la mode-awareness que le plancher vient de supprimer.
///
/// Idempotent (`create_dir_all` ; copy-or-overwrite ; merges non destructifs) et
/// **additif** — jamais de suppression (ADR-0031 §6). `trusted_root` : racine à
/// pré-approuver dans le `.claude.json` stagé ; `None` = pas de bloc `projects`
/// (le reste du plancher est tenu quand même).
pub(crate) fn prepare(
    home_root: &Path,
    sandbox_root: &Path,
    entries: &[String],
    run_id: &str,
    trusted_root: Option<&Path>,
) -> Result<PathBuf> {
    let staging = staging_dir_for_run(sandbox_root, run_id);
    let home = staged_claude_home(sandbox_root, run_id);
    std::fs::create_dir_all(&home)
        .with_context(|| format!("create staged claude home {}", home.display()))?;

    let src = home_root.join(".claude");
    let staged_json = staged_claude_json(sandbox_root, run_id);

    // -- PHASE 1 : matérialisation du profil ---------------------------------
    materialise_entries(home_root, &src, &home, &staged_json, &staging, entries)?;

    // -- PHASE 2 : plancher, profil-agnostique ------------------------------
    enforce_staging_floor(&src, &home, &staged_json, trusted_root)?;

    Ok(staging)
}

/// Phase 1 : une passe unique sur la liste gelée, chaque entrée routée par le
/// classificateur **unique** [`crate::sandbox_profile::landing`] — le même que
/// [`extra_mounts`] consulte, donc la vue *copie* et la vue *mount* ne peuvent pas
/// diverger.
///
/// N'inclut **aucune** garantie du plancher : celui-ci tourne juste après, quelle que
/// soit la liste. Une liste vide (`minimal`) est un no-op exact — la traduction
/// littérale de l'ancien bras `Mode::Minimal => {}`.
///
/// Politique manquant-sur-l'hôte : `warn!` + skip, pour les entrées du **défaut**
/// comme pour les **extras**. L'échec dur a été écarté (plan #432 D5) : il ferait
/// dépendre la politique de *qui a tapé le chemin* plutôt que de ce dont le conteneur
/// a besoin — sur une instance à Triggers horaires, désinstaller `gh` tuerait chaque
/// tir jusqu'à édition d'un profil. Et la règle M1 d'[`extra_mounts`] supprime le
/// danger réel (le répertoire root-owned), qui était la vraie justification du
/// fail-fast.
fn materialise_entries(
    home_root: &Path,
    src: &Path,
    home: &Path,
    staged_json: &Path,
    staging: &Path,
    entries: &[String],
) -> Result<()> {
    use crate::sandbox_profile::Landing;

    // `copy_root` du bloc `.claude` : la racine canonique `~/.claude`. Tout symlink
    // dont la cible sort de CET arbre est copié déréférencé (sa cible réelle n'est ni
    // copiée ni montée → danglerait, #409). Résolue une fois, partagée par toutes les
    // entrées `ClaudeHome` — c'est ce qui garde un lien `skills/x → ../agents/y` un
    // lien.
    let claude_copy_root = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());

    for entry in entries {
        match crate::sandbox_profile::landing(entry) {
            // Glob à un niveau (seul `.claude/*.md` en livre un) : capte CLAUDE.md et
            // ses imports siblings type RTK.md. Le motif est porté **tel quel** dans la
            // liste gelée, jamais expansé à la résolution — pas d'IO au chokepoint de
            // création, et la liste gelée reste byte-identique à ce que l'éditeur montre.
            Landing::ClaudeHome { rel, glob: true } => {
                copy_glob_one_level(src, home, rel)?;
            }
            Landing::ClaudeHome { rel, glob: false } => {
                let from = src.join(rel);
                let to = home.join(rel);
                stage_path(&from, &to, &claude_copy_root, entry)?;
            }
            // `.claude.json` sibling, verbatim (mode préservé). La confiance et
            // l'onboarding y sont mergés ensuite par le plancher (G4).
            Landing::ClaudeJson => {
                let from = home_root.join(".claude.json");
                if !from.exists() {
                    warn!(
                        "sandbox staging: profile entry `{entry}` is absent on the host \
                         ({}) — skipped",
                        from.display()
                    );
                } else {
                    copy_file_if_present(&from, staged_json)?;
                }
            }
            // Exception `$HOME` : copiée sous `<staging>/home/<rel>`, jamais bind-montée
            // depuis l'hôte (ADR-0031 §4). `copy_root` = la racine canonique de
            // **cette** entrée, pas `~/.claude` — sinon tout symlink interne à
            // `.config/gh` serait classé « échappant » et déréférencé.
            Landing::HomeExtra { rel } => {
                let from = home_root.join(rel);
                let to = staging.join("home").join(rel);
                let copy_root = std::fs::canonicalize(&from).unwrap_or_else(|_| from.clone());
                stage_path(&from, &to, &copy_root, entry)?;
            }
        }
    }
    Ok(())
}

/// Stage une entrée dont on ne sait pas a priori si c'est un fichier ou un dossier.
/// Dossier → walk préservant symlinks + bits exécutables ([`copy_tree_preserving`],
/// best-effort par entrée) ; fichier → [`std::fs::copy`] (mode préservé, dont 0600) ;
/// absent → `warn!` + skip.
fn stage_path(from: &Path, to: &Path, copy_root: &Path, entry: &str) -> Result<()> {
    let Ok(md) = std::fs::symlink_metadata(from) else {
        warn!(
            "sandbox staging: profile entry `{entry}` is absent on the host ({}) — skipped",
            from.display()
        );
        return Ok(());
    };
    // Un symlink top-level est suivi via `is_dir()`/`copy_file_if_present` : la cible
    // est ce que l'utilisateur a désigné.
    if md.file_type().is_symlink() || md.file_type().is_dir() {
        if from.is_dir() {
            copy_tree_preserving(from, to, copy_root, 0);
            return Ok(());
        }
        if !from.exists() {
            warn!(
                "sandbox staging: profile entry `{entry}` is a broken symlink ({}) — skipped",
                from.display()
            );
            return Ok(());
        }
    }
    copy_file_if_present(from, to)
}

/// Copie les entrées top-level de `src_dir` dont le nom matche `pattern` (un `*`
/// unique, ex. `*.md`) vers `dst_dir`. Glob **à un niveau**, sans dotglob implicite —
/// c'est exactement la sémantique de l'ancien `copy_top_level_md`. Un motif sans `*`
/// dégrade en égalité de nom. `src_dir` absent = no-op.
fn copy_glob_one_level(src_dir: &Path, dst_dir: &Path, pattern: &str) -> Result<()> {
    // Le motif ne porte qu'un segment (les entrées multi-segment à glob n'existent
    // pas dans le défaut) ; un motif imbriqué serait ignoré plutôt que mal interprété.
    if pattern.contains('/') {
        warn!("sandbox staging: nested glob entry `{pattern}` is not supported — skipped");
        return Ok(());
    }
    let (prefix, suffix) = match pattern.split_once('*') {
        Some((p, s)) => (p, s),
        None => (pattern, ""),
    };
    let Ok(entries) = std::fs::read_dir(src_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.len() < prefix.len() + suffix.len()
            || !name.starts_with(prefix)
            || !name.ends_with(suffix)
        {
            continue;
        }
        let from = entry.path();
        if from.is_file() {
            let to = dst_dir.join(entry.file_name());
            std::fs::copy(&from, &to).with_context(|| {
                format!("copy glob match {} -> {}", from.display(), to.display())
            })?;
        }
    }
    Ok(())
}

/// **Le plancher de staging** (#426, ADR-0031 §1) : les garanties que `prepare`
/// tient dans les **deux** modes — et demain quel que soit le profil. Chacune est
/// satisfaite soit par une **copie de l'hôte** (phase 1 ou ici), soit par une
/// **synthèse de repli**. Écrit exclusivement dans le staging ; ne touche jamais
/// l'hôte.
///
/// - **G1 credentials** — `.credentials.json` copié (0600 préservé par
///   [`std::fs::copy`]). Absent hôte → no-op (l'auth échouera plus loin, ce n'est
///   pas à `prepare` d'en juger).
/// - **G2 managed settings de l'org** — [`REMOTE_SETTINGS_FILE`] copié verbatim.
///   Absent hôte → `info!` + no-op (cas majoritaire : install sans org ; un `warn!`
///   par Run entraînerait à ignorer les warnings). Présent mais copie en échec →
///   erreur **dure** : c'est une surprise de conformité, pas un détail.
/// - **G3 bypass permissions** — [`BYPASS_PERMISSIONS_KEY`] à `true` dans le
///   `settings.json` stagé : **mergé** dans la copie hôte en `full`, **synthétisé**
///   en `minimal`. Fichier corrompu/non-objet → `warn!` + remplacé (symétrie avec
///   G4 : durcir ferait qu'un seul caractère malformé dans `~/.claude/settings.json`
///   empêcherait **tout** Run sandboxé).
/// - **G4 baseline `.claude.json`** — `hasCompletedOnboarding` (défaut défensif,
///   `or_insert`) + confiance sur `trusted_root` si fournie, 0600 réappliqué.
///   Inconditionnelle : `sandbox_container` monte ce chemin **toujours**, donc ne
///   rien écrire laisserait Docker créer un *répertoire* par-dessus
///   `$HOME/.claude.json`.
/// - **G5 puits de transcripts** — `projects/` créé **vide**.
///
/// Politique d'écriture : **fail-fast**. `prepare` tourne avant l'existence du
/// conteneur ; une garantie intenable doit échouer là, pas produire un Run qui pend
/// sur un dialogue sans personne devant. Ni le best-effort de
/// [`copy_tree_preserving`] (arbre de 1 Go volatil) ni l'avalage de
/// [`copy_jsonl_tree`] (transition terminale, ADR-0023) ne s'appliquent ici.
fn enforce_staging_floor(
    src: &Path,
    home: &Path,
    staged_json: &Path,
    trusted_root: Option<&Path>,
) -> Result<()> {
    // G1 — auth. En `minimal`, c'est la seule chose copiée de `~/.claude`
    // (`oauthAccount`/`userID` de `.claude.json` sont du cache profil PII inutile).
    copy_file_if_present(
        &src.join(".credentials.json"),
        &home.join(".credentials.json"),
    )?;

    // G2 — baseline de managed settings de l'org, writer UNIQUE (jamais l'allowlist).
    let remote_src = src.join(REMOTE_SETTINGS_FILE);
    if remote_src.exists() {
        let remote_dst = home.join(REMOTE_SETTINGS_FILE);
        std::fs::copy(&remote_src, &remote_dst).with_context(|| {
            format!(
                "stage org managed settings {} -> {}",
                remote_src.display(),
                remote_dst.display()
            )
        })?;
    } else {
        // Cas majoritaire (install sans organisation). `info!` et non `debug!` : le
        // filtre par défaut du daemon est `pdo_daemon=info,info` (`main.rs`).
        info!(
            "sandbox staging: no {REMOTE_SETTINGS_FILE} at {} — nothing to consent to (no-op)",
            remote_src.display()
        );
    }

    // G3 — bypass permissions. `insert` (pas `entry().or_insert()`) : un `false`
    // hôte doit être écrasé, sinon l'agent se bloque sur le prompt. Pas de `chmod`
    // (ce n'est pas un secret ; `fs::write` tronque en place et préserve donc le
    // mode hôte en `full`) et pas de tmp+rename : aucun lecteur concurrent n'existe
    // avant le conteneur — contrairement à `merge_back`/`atomic_copy_into`.
    edit_json_object(&home.join(SETTINGS_FILE), "staged settings.json", |obj| {
        obj.insert(BYPASS_PERMISSIONS_KEY.to_string(), serde_json::json!(true));
    })?;

    // G4 — baseline du `.claude.json` stagé (onboarding + confiance).
    ensure_claude_json_baseline(staged_json, trusted_root)?;

    // G5 — `projects/` créé VIDE (puits de transcripts runtime). Ni
    // `~/.claude/projects/` ni le sous-dir encodé n'existent pour un run frais.
    let projects = home.join("projects");
    std::fs::create_dir_all(&projects)
        .with_context(|| format!("create staged projects sink {}", projects.display()))?;

    Ok(())
}

/// Récupère les transcripts (`projects/**/*.jsonl`) du staging vers
/// `<home_root>/.claude/projects/`, **récursivement** (transcripts de sessions
/// *et* de sous-agents `<uuid>/subagents/*.jsonl`), sous le même dirname encodé.
///
/// Idempotent : copie ssi le fichier hôte est **absent** OU **strictement plus
/// petit** (transcripts append-only ⇒ `staging > hôte ⇔ contenu nouveau`). Ne
/// réécrit jamais un fichier hôte `>=`, n'écrit rien hors `projects/`.
///
/// **Best-effort** : tolère tout échec `read_dir`/`copy` sans jamais faire échouer
/// l'appelant (la transition terminale du Run ne doit pas dépendre de ce merge).
/// `projects/` staging absent (run sans session) = no-op propre.
pub(crate) fn merge_back(home_root: &Path, sandbox_root: &Path, run_id: &str) -> Result<()> {
    let src = staged_claude_home(sandbox_root, run_id).join("projects");
    let dest = home_root.join(".claude").join("projects");
    if !src.is_dir() {
        return Ok(()); // rien écrit
    }
    let Ok(entries) = std::fs::read_dir(&src) else {
        return Ok(());
    };
    // Un Run = plusieurs dirs encodés (un par worktree de node, manager,
    // merge-resolver). Itérer TOUS les sous-dossiers, jamais supposer un seul.
    for entry in entries.flatten() {
        let proj = entry.path();
        if proj.is_dir() {
            copy_jsonl_tree(&proj, &dest.join(entry.file_name()));
        }
    }
    Ok(())
}

/// Supprime `<sandbox_root>/<run_id>/`. No-op si absent (best-effort, miroir de
/// [`crate::worktree_ops::reap_orphan_sub_worktree`]).
pub(crate) fn teardown(sandbox_root: &Path, run_id: &str) -> Result<()> {
    let _ = std::fs::remove_dir_all(staging_dir_for_run(sandbox_root, run_id));
    Ok(())
}

// -- résolveur de bord (unique lecture HOME) ---------------------------------

/// `(home_root, sandbox_root)` = `($HOME, $HOME/.pdo/sandbox)`. `None` si `HOME`
/// est absent. Câblé par le daemon (#407) ; les unit tests injectent des temp
/// dirs et bypassent ce résolveur (pas de swap `HOME` → pas de mutex global).
pub(crate) fn default_roots_from_env() -> Option<(PathBuf, PathBuf)> {
    let home = PathBuf::from(std::env::var("HOME").ok()?);
    let sandbox = home.join(".pdo").join("sandbox");
    Some((home, sandbox))
}

// -- helpers (privés) --------------------------------------------------------

/// Copie `src` → `dst` s'il existe (fichier de l'allowlist absent = no-op).
/// [`std::fs::copy`] préserve le mode sous Unix (dont 0600 des credentials), même
/// en écrasant une destination existante.
fn copy_file_if_present(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    std::fs::copy(src, dst)
        .with_context(|| format!("copy {} -> {}", src.display(), dst.display()))?;
    Ok(())
}

/// *Lire-comme-objet-ou-dégrader → muter → écrire pretty.* La **politique de
/// dégradation unique** du module (#426) : fichier absent ou illisible → objet vide
/// (synthèse) ; objet → merge ; JSON valide mais **pas** un objet, ou corrompu →
/// `warn!` + objet vide. `what` nomme le fichier dans les logs et les erreurs.
///
/// Volontairement sans paramètre de mode de fichier : la confidentialité reste au
/// callsite (0600 pour le `.claude.json`, rien pour le `settings.json`), là où
/// l'asymétrie est décidée.
fn edit_json_object(
    path: &Path,
    what: &str,
    mutate: impl FnOnce(&mut serde_json::Map<String, serde_json::Value>),
) -> Result<()> {
    let mut value = match std::fs::read_to_string(path) {
        Ok(body) => serde_json::from_str(&body).unwrap_or_else(|e| {
            warn!(
                "sandbox staging: {what} at {} is not valid JSON ({e}); replacing with a synthesised object",
                path.display()
            );
            serde_json::json!({})
        }),
        Err(_) => serde_json::json!({}),
    };
    if !value.is_object() {
        warn!(
            "sandbox staging: {what} at {} is not a JSON object; replacing with a synthesised object",
            path.display()
        );
        value = serde_json::json!({});
    }
    let obj = value.as_object_mut().expect("value forced to object above");
    mutate(obj);

    let body = serde_json::to_string_pretty(&value).with_context(|| format!("serialize {what}"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    std::fs::write(path, body).with_context(|| format!("write {what} {}", path.display()))?;
    Ok(())
}

/// Garantie **G4** du plancher : le `.claude.json` stagé porte
/// `hasCompletedOnboarding` et, si `trusted_root` est fournie, la confiance de cette
/// racine (héritée par les descendants → couvre les worktrees de nodes). Merge non
/// destructif dans le fichier hôte copié en `full` (profil `oauthAccount`, autres
/// repos trustés… préservés) ; synthèse en `minimal`. 0600 réappliqué (le fichier
/// peut porter un token).
///
/// `hasCompletedOnboarding` en `or_insert` (**défaut défensif** : une valeur hôte
/// existante est respectée) mais les deux drapeaux de confiance en `insert` (ce sont
/// des **garanties** : un `false` bloquerait un Run autonome).
fn ensure_claude_json_baseline(path: &Path, trusted_root: Option<&Path>) -> Result<()> {
    edit_json_object(path, "staged .claude.json", |obj| {
        obj.entry("hasCompletedOnboarding")
            .or_insert_with(|| serde_json::json!(true));
        let Some(root) = trusted_root else {
            return; // pas de bloc `projects` — objet nu (hors `full`, où l'hôte le porte)
        };
        let projects = obj
            .entry("projects")
            .or_insert_with(|| serde_json::json!({}));
        if !projects.is_object() {
            *projects = serde_json::json!({});
        }
        let projects = projects.as_object_mut().expect("projects forced to object");
        let key = root.to_string_lossy().into_owned();
        let entry = projects.entry(key).or_insert_with(|| serde_json::json!({}));
        if !entry.is_object() {
            *entry = serde_json::json!({});
        }
        let entry = entry
            .as_object_mut()
            .expect("project entry forced to object");
        entry.insert(
            "hasTrustDialogAccepted".to_string(),
            serde_json::json!(true),
        );
        entry.insert(
            "hasCompletedProjectOnboarding".to_string(),
            serde_json::json!(true),
        );
    })?;
    set_mode_0600(path)
}

/// `chmod 0600` (le `.claude.json` généré contient un token potentiel côté
/// conteneur ; [`std::fs::write`] laisserait 0644 par défaut).
fn set_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))
}

/// Profondeur de récursion max du walk `copy` — garde-fou défensif contre un cycle
/// pathologique de symlinks **échappants** (les liens intra-arbre sont recréés,
/// jamais suivis, donc ne peuvent pas boucler ; seuls les échappants déréférencés
/// récursent dans un arbre étranger). Un vrai `~/.claude` fait quelques niveaux.
const MAX_COPY_DEPTH: u32 = 64;

/// Walk `src` → `dst` en **préservant les bits exécutables** et en traitant les
/// symlinks selon qu'ils **restent dans** l'arbre copié ou en **sortent** (voir
/// [`stage_symlink`]). Marche par entrée via [`std::fs::symlink_metadata`] (ne suit
/// PAS les liens) :
/// - symlink → [`stage_symlink`] (verbatim si intra-`copy_root`, déréférencé sinon) ;
/// - dir → `create_dir_all` + récursion ;
/// - file → [`std::fs::copy`] (préserve le mode/exec bit gratis sous Unix) ;
/// - autre (socket/fifo/device) → skip.
///
/// **Best-effort par entrée** (#409, D3) : toute op ratée (`create_dir`, `read_dir`,
/// `stat`, `copy`, `symlink`) est loggée en `warn!` et **sautée** — la fonction ne
/// remonte jamais d'erreur. Un `~/.claude` volumineux et volatil (autres process
/// Claude qui mutent `node_modules`) ne doit pas faire échouer le Run.
///
/// `copy_root` = racine (canonique) au-delà de laquelle une cible de symlink est
/// jugée « échappante ». `depth` = profondeur courante ([`MAX_COPY_DEPTH`]).
///
/// N.B. : ne PAS réutiliser `copy_dir_all` de `lib.rs` — il n'est pas
/// symlink-aware (`std::fs::copy` déréférence).
fn copy_tree_preserving(src: &Path, dst: &Path, copy_root: &Path, depth: u32) {
    if depth > MAX_COPY_DEPTH {
        warn!(
            "sandbox copy: max depth {MAX_COPY_DEPTH} exceeded at {}, skipping subtree",
            src.display()
        );
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dst) {
        warn!(
            "sandbox copy: cannot create {} ({e:#}); skipping subtree",
            dst.display()
        );
        return;
    }
    let Ok(entries) = std::fs::read_dir(src) else {
        warn!(
            "sandbox copy: cannot read dir {}; skipping subtree",
            src.display()
        );
        return;
    };
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let Ok(md) = std::fs::symlink_metadata(&from) else {
            warn!(
                "sandbox copy: cannot stat {}; skipping entry",
                from.display()
            );
            continue;
        };
        let ft = md.file_type();
        if ft.is_symlink() {
            stage_symlink(&from, &to, copy_root, depth);
        } else if ft.is_dir() {
            copy_tree_preserving(&from, &to, copy_root, depth + 1);
        } else if ft.is_file() {
            if let Err(e) = std::fs::copy(&from, &to) {
                warn!(
                    "sandbox copy: skip file {} -> {} ({e:#})",
                    from.display(),
                    to.display()
                );
            }
        }
        // else : socket/fifo/device → skip silencieux.
    }
}

/// Traite une entrée symlink d'un walk `copy`. Une cible **intra-arbre** (toujours
/// sous `copy_root`, ex. `../semver/bin/x` ou un sibling) est recréée **verbatim** —
/// préserve les cycles `node_modules/.bin` et les liens relatifs. Une cible qui
/// **échappe** `copy_root` (ex. un skill lié à `~/.agents/…`, ou un lien absolu)
/// **danglerait** dans le conteneur (sa cible réelle n'est ni copiée ni montée) →
/// on **déréférence** : le contenu réel est copié à la place. La cible déréférencée
/// devient son propre `copy_root` pour que ses liens internes qui y restent restent
/// des liens. Cible cassée / illisible → skip (best-effort, jamais d'échec).
fn stage_symlink(from: &Path, to: &Path, copy_root: &Path, depth: u32) {
    let Ok(link_target) = std::fs::read_link(from) else {
        warn!("sandbox copy: skip unreadable symlink {}", from.display());
        return;
    };
    // Résoudre la cible en chemin absolu (relatif → joint au parent du lien) puis
    // canonicaliser : sert à tester l'échappement ET à atteindre le contenu réel.
    // Un lien cassé/bouclant fait échouer `canonicalize` → skip (jamais d'échec).
    let resolved = if link_target.is_absolute() {
        link_target.clone()
    } else {
        match from.parent() {
            Some(parent) => parent.join(&link_target),
            None => link_target.clone(),
        }
    };
    let Ok(canonical) = std::fs::canonicalize(&resolved) else {
        warn!(
            "sandbox copy: skip broken symlink {} -> {}",
            from.display(),
            link_target.display()
        );
        return;
    };
    if canonical.starts_with(copy_root) {
        // Intra-arbre → recréer le lien verbatim (cible d'origine, non résolue).
        let _ = std::fs::remove_file(to);
        if let Err(e) = std::os::unix::fs::symlink(&link_target, to) {
            warn!(
                "sandbox copy: skip symlink {} -> {} ({e:#})",
                to.display(),
                link_target.display()
            );
        }
    } else if canonical.is_dir() {
        // Échappant (dossier) → déréférencer ; la cible réelle est son copy_root.
        copy_tree_preserving(&canonical, to, &canonical, depth + 1);
    } else if canonical.is_file() {
        // Échappant (fichier) → copier le contenu déréférencé (mode préservé).
        let _ = std::fs::remove_file(to);
        if let Err(e) = std::fs::copy(&canonical, to) {
            warn!(
                "sandbox copy: skip deref file {} -> {} ({e:#})",
                canonical.display(),
                to.display()
            );
        }
    }
    // else : échappant non-régulier (socket/fifo) → skip.
}

/// Recopie récursivement les `*.jsonl` de `src_dir` vers `dest_dir`, copy-if-
/// absent-or-larger, atomiquement. Miroir de
/// [`crate::run_cost`]'s `collect_jsonl_recursive` (même prédicat `is_dir` +
/// extension `jsonl`) pour que le copy-set égale le read-set du coût.
/// Best-effort : tout échec est avalé (jamais de propagation vers l'appelant).
fn copy_jsonl_tree(src_dir: &Path, dest_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(src_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let from = entry.path();
        if from.is_dir() {
            // Préserve le sous-arbre relatif (`<uuid>/subagents/agent-X.jsonl`),
            // jamais un aplatissement par basename (sinon collisions de sessions).
            copy_jsonl_tree(&from, &dest_dir.join(entry.file_name()));
        } else if from.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            // `create_dir_all` paresseux : seulement quand un `.jsonl` existe.
            if std::fs::create_dir_all(dest_dir).is_err() {
                continue;
            }
            let dst = dest_dir.join(entry.file_name());
            let should_copy = match std::fs::metadata(&dst) {
                Err(_) => true, // absent côté hôte
                Ok(dst_md) => {
                    let src_len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    src_len > dst_md.len() // append-only ⇒ plus grand = contenu nouveau
                }
            };
            if should_copy {
                let _ = atomic_copy_into(&from, &dst, dest_dir);
            }
        }
    }
}

/// Compteur monotone pour des noms de fichiers temporaires uniques (évite qu'un
/// merge concurrent — modal Stats calculant le coût, resume — collisionne).
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Copie atomique de `src` vers `dst` : `tmp` dans `dest_dir` (même filesystem)
/// puis `rename`. Un `compute_run_cost` concurrent ne doit jamais lire une ligne
/// à moitié écrite — `fs::copy` seul n'est pas atomique. Le `tmp` ne finit pas en
/// `.jsonl` → le lecteur de coût l'ignore même s'il apparaît transitoirement.
fn atomic_copy_into(src: &Path, dst: &Path, dest_dir: &Path) -> Result<()> {
    let base = dst
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "transcript".to_string());
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = dest_dir.join(format!("{base}.pdo-merge-tmp.{}.{seq}", std::process::id()));
    std::fs::copy(src, &tmp)
        .with_context(|| format!("copy {} -> {}", src.display(), tmp.display()))?;
    match std::fs::rename(&tmp, dst) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp); // ne pas laisser d'orphelin
            Err(e).with_context(|| format!("rename {} -> {}", tmp.display(), dst.display()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    // Un dirname encodé réaliste (cf. `stale_detector::encode_working_dir`) —
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
        let base = crate::sandbox_profile::base_entries(crate::sandbox_profile::FULL_PROFILE);
        crate::sandbox_profile::resolve_entry_list(&base, &[], &[]).entries
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
        // plancher (G2) dans les DEUX modes. Miroir de la fixture couche 3
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

    // -- path math -----------------------------------------------------------

    #[test]
    fn staging_dir_for_run_follows_canonical_schema() {
        let sandbox = Path::new("/home/u/.pdo/sandbox");
        assert_eq!(
            staging_dir_for_run(sandbox, "run-x"),
            PathBuf::from("/home/u/.pdo/sandbox/run-x")
        );
        assert_eq!(
            staged_claude_home(sandbox, "run-x"),
            PathBuf::from("/home/u/.pdo/sandbox/run-x/claude-home")
        );
        assert_eq!(
            staged_claude_json(sandbox, "run-x"),
            PathBuf::from("/home/u/.pdo/sandbox/run-x/.claude.json")
        );
    }

    // -- prepare (full) ------------------------------------------------------

    #[test]
    fn prepare_full_reproduces_allowlist_and_excludes_projects() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        let staging = prepare(
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

        prepare(
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
        prepare(
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
        // `$HOME/.claude.json`. Le plancher ferme ce footgun.
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

        prepare(
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
        prepare(
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

        prepare(
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

    // -- prepare (minimal) ---------------------------------------------------

    /// *L'*assertion que `minimal` == le plancher, ni plus ni moins (#426) : le
    /// contenu exact de `claude-home/` est le plancher, alors que l'hôte porte tout
    /// l'appareil `full` (skills, plugins, settings riches…).
    #[test]
    fn prepare_minimal_stages_only_the_floor() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path()); // skills/settings existent → prouvent l'exclusion

        prepare(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Ensemble EXACT (trié ASCII) = les fichiers du plancher, rien d'autre.
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
        // porte des `hooks`) — c'est la fourche synthèse-vs-copie du plancher.
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

        prepare(
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

        prepare(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(staged_claude_json(sandbox_dir.path(), "run1")).unwrap(),
        )
        .unwrap();
        // Exactement une clé : hasCompletedOnboarding.
        assert_eq!(json, serde_json::json!({ "hasCompletedOnboarding": true }));
    }

    // -- plancher de staging, G2 : managed settings de l'org (#426) -----------

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

        prepare(
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

        prepare(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

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
        prepare(
            home_dir.path(),
            sandbox_dir.path(),
            &full_entries(),
            "run1",
            None,
        )
        .unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!home.join(REMOTE_SETTINGS_FILE).exists());
        // …et le plancher n'a PAS avorté en cours de route.
        assert!(home.join(SETTINGS_FILE).is_file());
        assert!(home.join("projects").is_dir());
    }

    #[test]
    fn prepare_minimal_without_remote_settings_is_a_logged_noop() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home_without_org(home_dir.path());

        prepare(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!home.join(REMOTE_SETTINGS_FILE).exists());
        assert!(home.join(SETTINGS_FILE).is_file());
        assert!(home.join("projects").is_dir());
        // G1 tenue malgré le no-op de G2.
        assert_eq!(mode_of(&home.join(".credentials.json")), 0o600);
    }

    // -- plancher de staging, G3 : bypass permissions (#426) ------------------

    fn staged_settings(sandbox: &Path, run_id: &str) -> PathBuf {
        staged_claude_home(sandbox, run_id).join(SETTINGS_FILE)
    }

    #[test]
    fn prepare_full_merges_bypass_into_host_settings() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let host_settings = home_dir.path().join(".claude").join(SETTINGS_FILE);
        write(&host_settings, r#"{"hooks":{"Stop":[]},"model":"opus"}"#);

        prepare(
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

        prepare(
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

        prepare(
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

        prepare(
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

        prepare(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

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
        prepare(
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

        prepare(
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

    // -- plancher de staging, les CINQ garanties en un point (#426) -----------

    /// La forme exécutable d'ADR-0031 §1, et le test qu'un relecteur lit pour
    /// répondre « le plancher est-il tenu ? ». La slice « profils » l'étendra
    /// (même corps, plus « … même avec l'entrée décochée »).
    #[test]
    fn prepare_floor_holds_in_both_modes() {
        // #432: "both modes" is now "the two virtual defaults' resolved lists", plus a
        // THIRD case that only profiles make reachable — `full` with the class-(b) entry
        // `.claude/settings.json` UNCHECKED. That case is the whole reason ADR-0031 §1
        // states the floor as guarantees rather than files: G3 must still hold, by
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

            prepare(
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
    fn prepare_twice_keeps_the_floor_and_is_additive() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());
        let trusted = Path::new("/repo/root");
        let (home_p, sandbox_p) = (home_dir.path(), sandbox_dir.path());

        prepare(home_p, sandbox_p, &full_entries(), "run1", Some(trusted)).unwrap();
        // Le conteneur (simulé) a écrit un transcript dans le puits.
        stage_transcript(sandbox_p, "run1", &format!("{ENC}/s.jsonl"), "line\n");

        prepare(home_p, sandbox_p, &full_entries(), "run1", Some(trusted)).unwrap();

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

    // -- merge_back ----------------------------------------------------------

    // -- #432 : entrées d'exception `$HOME` (copie + queue de mounts) ---------

    /// ADR-0031 §4, la moitié *copie* : une entrée hors `.claude` atterrit sous
    /// `<staging>/home/<rel>`, jamais ailleurs — et surtout jamais dans `claude-home/`,
    /// où elle finirait à `$HOME/.claude/<rel>` côté conteneur.
    #[test]
    fn prepare_copies_a_home_extra_under_staging_home() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        let entries = vec![".gitconfig".to_string(), ".config/gh".to_string()];
        prepare(home_dir.path(), sandbox_dir.path(), &entries, "run1", None).unwrap();

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

        prepare(
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

        prepare(
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

        prepare(home_dir.path(), sandbox_dir.path(), &entries, "run1", None).unwrap();

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

        prepare(
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
    /// propre, tout le plancher.
    #[test]
    fn an_empty_entry_list_stages_the_floor_and_nothing_else() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare(home_dir.path(), sandbox_dir.path(), &[], "run1", None).unwrap();

        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(!home.join("skills").exists());
        assert!(!home.join("CLAUDE.md").exists());
        assert!(!staged_home_extras(sandbox_dir.path(), "run1").exists());
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

    // -- teardown ------------------------------------------------------------

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
        // No-op idempotent : purge d'un run inexistant.
        teardown(sandbox_dir.path(), "never-created").unwrap();
    }

    // -- round-trip prepare → (write) → merge_back → teardown ----------------

    #[test]
    fn prepare_minimal_then_merge_and_teardown_roundtrip() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        prepare(home, sandbox, &[], "run1", None).unwrap();
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
}
