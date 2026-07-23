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
//! - #407 câble `prepare`/`merge_back`/`teardown` dans le run-advance (ADR-0030) ;
//! - #408 pointe stale-detection/coût vers le staging (seam `transcripts_root`).
//!
//! ## Décisions de conception (voir la section « Sandbox » de `CONTEXT.md`)
//! - **`copy` = allowlist, jamais denylist.** Copier « tout `~/.claude` sauf
//!   `projects/` » embarquerait ~98 Mo d'état hôte (`history.jsonl`,
//!   `session-env/`, `file-history/`…) — fuite d'isolation + fragile aux futures
//!   versions de Claude Code. On copie une liste explicite.
//! - **`merge_back` récurse.** ~42 % des transcripts vivent dans
//!   `projects/<enc>/<uuid>/subagents/*.jsonl` (profondeur 9). Le copy-set doit
//!   *égaler* le read-set de [`crate::run_cost`] (`collect_jsonl_recursive`),
//!   sinon le coût des runs sandboxés est sous-estimé (régression silencieuse).
//! - **`pure` seede la confiance.** `{"hasCompletedOnboarding":true}` seul
//!   désarme l'onboarding mais PAS le dialogue « trust this folder ? » — un agent
//!   non surveillé se bloquerait. On pré-approuve la racine du Run.

#![allow(dead_code)] // Tracer bullet : consommé par #406/#407, non câblé dans cette slice.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};

/// Comment le *staged Claude home* d'un Run est seedé. `off` (PRD) n'est PAS une
/// variante : le caller skippe simplement [`prepare`] (pas de branche no-op dans
/// la couche fs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Mode {
    Copy,
    Pure,
}

/// Fichiers de config top-level de `~/.claude` recopiés en mode `copy` (en plus
/// des `*.md` captés par glob). `.credentials.json` préserve son mode 0600 via
/// [`std::fs::copy`].
const COPY_ALLOWLIST_FILES: &[&str] = &["settings.json", "settings.local.json", ".credentials.json"];

/// Dossiers de `~/.claude` recopiés en mode `copy` (walk préservant symlinks +
/// bits exécutables). `projects/` est délibérément absent : jamais copié.
const COPY_ALLOWLIST_DIRS: &[&str] = &["skills", "plugins", "agents", "commands", "output-styles"];

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

// -- effets (sync std::fs, anyhow + .context) --------------------------------

/// Seede le *staged Claude home* et renvoie sa racine (`<sandbox_root>/<run_id>`).
///
/// Idempotent (`create_dir_all` ; copy-or-overwrite). `trusted_root` : racine à
/// pré-approuver dans le `.claude.json` en mode `pure` (`None` = objet nu). En
/// mode `copy`, `trusted_root` est ignoré (la confiance hôte est copiée verbatim).
pub(crate) fn prepare(
    home_root: &Path,
    sandbox_root: &Path,
    mode: Mode,
    run_id: &str,
    trusted_root: Option<&Path>,
) -> Result<PathBuf> {
    let staging = staging_dir_for_run(sandbox_root, run_id);
    let home = staged_claude_home(sandbox_root, run_id);
    std::fs::create_dir_all(&home)
        .with_context(|| format!("create staged claude home {}", home.display()))?;

    let src = home_root.join(".claude");

    match mode {
        Mode::Copy => {
            // Allowlist top-level : `*.md` (capte CLAUDE.md + imports siblings type
            // RTK.md) + les fichiers de config nommés (0600 des creds préservé).
            copy_top_level_md(&src, &home)?;
            for name in COPY_ALLOWLIST_FILES {
                copy_file_if_present(&src.join(name), &home.join(name))?;
            }
            // Allowlist dirs : walk préservant symlinks + mode. `projects/` EXCLU.
            for name in COPY_ALLOWLIST_DIRS {
                let dir_src = src.join(name);
                if dir_src.is_dir() {
                    copy_tree_preserving(&dir_src, &home.join(name))?;
                }
            }
            // `.claude.json` sibling, verbatim (mode préservé). Chemin explicite :
            // c'est un dotfile, un glob `*` sans dotglob le raterait.
            copy_file_if_present(
                &home_root.join(".claude.json"),
                &staged_claude_json(sandbox_root, run_id),
            )?;
        }
        Mode::Pure => {
            // Auth = `.credentials.json` seul (`oauthAccount`/`userID` de
            // `.claude.json` sont du cache profil PII inutile).
            copy_file_if_present(
                &src.join(".credentials.json"),
                &home.join(".credentials.json"),
            )?;
            write_pure_claude_json(&staged_claude_json(sandbox_root, run_id), trusted_root)?;
        }
    }

    // Les deux modes : `projects/` créé VIDE (puits de transcripts runtime). Ni
    // `~/.claude/projects/` ni le sous-dir encodé n'existent pour un run frais.
    let projects = home.join("projects");
    std::fs::create_dir_all(&projects)
        .with_context(|| format!("create staged projects sink {}", projects.display()))?;

    Ok(staging)
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
/// `projects/` staging absent (mode `pure`, run sans session) = no-op propre.
pub(crate) fn merge_back(home_root: &Path, sandbox_root: &Path, run_id: &str) -> Result<()> {
    let src = staged_claude_home(sandbox_root, run_id).join("projects");
    let dest = home_root.join(".claude").join("projects");
    if !src.is_dir() {
        return Ok(()); // pure / rien écrit
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

/// Copie les `*.md` top-level de `src_dir` vers `dst_dir` (glob à un niveau).
/// Tolérant : `src_dir` absent = no-op. Capte `CLAUDE.md` et ses imports siblings
/// (`RTK.md`, …). Le mode est préservé par [`std::fs::copy`].
fn copy_top_level_md(src_dir: &Path, dst_dir: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(src_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().ends_with(".md") {
            continue;
        }
        let from = entry.path();
        if from.is_file() {
            let to = dst_dir.join(&name);
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy md {} -> {}", from.display(), to.display()))?;
        }
    }
    Ok(())
}

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

/// Écrit le `.claude.json` du mode `pure` (chmod 0600). `Some(root)` seede la
/// confiance de la racine (héritée par les descendants → couvre les worktrees de
/// nodes) ; `None` → objet nu `{"hasCompletedOnboarding":true}`.
fn write_pure_claude_json(dst: &Path, trusted_root: Option<&Path>) -> Result<()> {
    let value = match trusted_root {
        Some(root) => {
            let key = root.to_string_lossy().into_owned();
            serde_json::json!({
                "hasCompletedOnboarding": true,
                "projects": {
                    key: {
                        "hasTrustDialogAccepted": true,
                        "hasCompletedProjectOnboarding": true,
                    }
                }
            })
        }
        None => serde_json::json!({ "hasCompletedOnboarding": true }),
    };
    let body = serde_json::to_string_pretty(&value).context("serialize pure .claude.json")?;
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create parent {}", parent.display()))?;
    }
    std::fs::write(dst, body)
        .with_context(|| format!("write pure .claude.json {}", dst.display()))?;
    set_mode_0600(dst)?;
    Ok(())
}

/// `chmod 0600` (le `.claude.json` généré contient un token potentiel côté
/// conteneur ; [`std::fs::write`] laisserait 0644 par défaut).
fn set_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))
}

/// Walk `src` → `dst` en **préservant symlinks et bits exécutables**. Marche par
/// entrée via [`std::fs::symlink_metadata`] (ne suit PAS les liens) :
/// - symlink → recréé verbatim (jamais déréférencé : cycles `node_modules/.bin`,
///   liens cassés, bloat `~/.agents` inlinés) ;
/// - dir → `create_dir_all` + récursion ;
/// - file → [`std::fs::copy`] (préserve le mode/exec bit gratis sous Unix) ;
/// - autre (socket/fifo/device) → skip.
///
/// N.B. : ne PAS réutiliser `copy_dir_all` de `lib.rs` — il n'est pas
/// symlink-aware (`std::fs::copy` déréférence).
fn copy_tree_preserving(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst).with_context(|| format!("create dir {}", dst.display()))?;
    let entries =
        std::fs::read_dir(src).with_context(|| format!("read dir {}", src.display()))?;
    for entry in entries.flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        let md = std::fs::symlink_metadata(&from)
            .with_context(|| format!("stat {}", from.display()))?;
        let ft = md.file_type();
        if ft.is_symlink() {
            let target =
                std::fs::read_link(&from).with_context(|| format!("read_link {}", from.display()))?;
            // Idempotence : une exécution antérieure a pu laisser un lien/fichier.
            let _ = std::fs::remove_file(&to);
            std::os::unix::fs::symlink(&target, &to).with_context(|| {
                format!("symlink {} -> {}", to.display(), target.display())
            })?;
        } else if ft.is_dir() {
            copy_tree_preserving(&from, &to)?;
        } else if ft.is_file() {
            std::fs::copy(&from, &to)
                .with_context(|| format!("copy {} -> {}", from.display(), to.display()))?;
        }
        // else : socket/fifo/device → skip silencieux.
    }
    Ok(())
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
        std::fs::symlink_metadata(path).unwrap().permissions().mode() & 0o777
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn write_mode(path: &Path, content: &str, mode: u32) {
        write(path, content);
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// Construit un faux `~/.claude` réaliste sous `<home>/.claude` + le sibling
    /// `<home>/.claude.json`, couvrant l'allowlist, un symlink, un exécutable,
    /// des creds 0600, ET de l'état hôte volumineux qui doit rester EXCLU.
    fn fabricate_home(home: &Path) {
        let claude = home.join(".claude");
        // Allowlist dirs.
        write(&claude.join("skills/foo/skill.md"), "# skill\n");
        write_mode(&claude.join("skills/foo/run.sh"), "#!/bin/sh\necho hi\n", 0o755);
        // Symlink relatif à l'intérieur de skills/foo → skill.md.
        std::os::unix::fs::symlink("skill.md", claude.join("skills/foo/link.md")).unwrap();
        write(&claude.join("plugins/bar/plugin.json"), "{}\n");
        write(&claude.join("agents/a.md"), "agent\n");
        write(&claude.join("commands/c.md"), "cmd\n");
        write(&claude.join("output-styles/s.md"), "style\n");
        // Allowlist files.
        write(&claude.join("settings.json"), r#"{"hooks":{"Stop":[]}}"#);
        write(&claude.join("settings.local.json"), r#"{"local":true}"#);
        write_mode(&claude.join(".credentials.json"), r#"{"token":"secret"}"#, 0o600);
        write(&claude.join("CLAUDE.md"), "# global\n");
        write(&claude.join("RTK.md"), "# rtk\n");
        // État hôte volumineux — DOIT rester exclu.
        write(&claude.join("history.jsonl"), "{\"cmd\":\"ls\"}\n");
        write(&claude.join("file-history/big.bin"), "xxxxxxxxxx");
        write(&claude.join("session-env/env-1/data"), "junk");
        // Transcripts hôte pré-existants — NE doivent PAS être copiés par prepare.
        write(&claude.join(format!("projects/{ENC}/old.jsonl")), "{\"host\":1}\n");
        // Sibling `.claude.json`.
        write(&home.join(".claude.json"), r#"{"host":"profile","oauthAccount":{"x":1}}"#);
    }

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

    // -- prepare (copy) ------------------------------------------------------

    #[test]
    fn prepare_copy_reproduces_allowlist_and_excludes_projects() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        let staging =
            prepare(home_dir.path(), sandbox_dir.path(), Mode::Copy, "run1", None).unwrap();
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
        assert!(settings.contains("hooks"), "hooks vivent dans settings.json");
        assert!(home.join("settings.local.json").is_file());
        assert!(home.join(".credentials.json").is_file());
        assert!(home.join("CLAUDE.md").is_file());
        assert!(home.join("RTK.md").is_file(), "*.md siblings captés par glob");

        // `.claude.json` sibling copié verbatim (hors claude-home/).
        let staged_json = staged_claude_json(sandbox_dir.path(), "run1");
        assert!(staged_json.is_file());
        assert_eq!(
            std::fs::read_to_string(&staged_json).unwrap(),
            r#"{"host":"profile","oauthAccount":{"x":1}}"#
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
    fn prepare_copy_preserves_symlinks_and_exec_bit() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path());

        prepare(home_dir.path(), sandbox_dir.path(), Mode::Copy, "run1", None).unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // Symlink recréé COMME lien, cible verbatim.
        let link = home.join("skills/foo/link.md");
        let md = std::fs::symlink_metadata(&link).unwrap();
        assert!(md.file_type().is_symlink(), "le lien doit rester un symlink");
        assert_eq!(std::fs::read_link(&link).unwrap(), PathBuf::from("skill.md"));

        // Exec bit conservé.
        assert_eq!(mode_of(&home.join("skills/foo/run.sh")), 0o755);
        // 0600 des creds préservé.
        assert_eq!(mode_of(&home.join(".credentials.json")), 0o600);
    }

    #[test]
    fn prepare_copy_ignores_missing_entries() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        // Home minimal : uniquement settings.json, aucun autre membre de l'allowlist.
        write(&home_dir.path().join(".claude/settings.json"), "{}");

        // Ne doit pas paniquer / échouer sur les entrées absentes.
        prepare(home_dir.path(), sandbox_dir.path(), Mode::Copy, "run1", None).unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");
        assert!(home.join("settings.json").is_file());
        assert!(!home.join("skills").exists());
        assert!(!home.join(".credentials.json").exists());
        assert!(!staged_claude_json(sandbox_dir.path(), "run1").exists());
        assert!(home.join("projects").is_dir());
    }

    // -- prepare (pure) ------------------------------------------------------

    #[test]
    fn prepare_pure_only_credentials_and_onboarding() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        fabricate_home(home_dir.path()); // skills/settings existent → prouvent l'exclusion

        prepare(home_dir.path(), sandbox_dir.path(), Mode::Pure, "run1", None).unwrap();
        let home = staged_claude_home(sandbox_dir.path(), "run1");

        // claude-home ne contient QUE `.credentials.json` + `projects/` (vide).
        let mut names: Vec<String> = std::fs::read_dir(&home)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(names, vec![".credentials.json".to_string(), "projects".to_string()]);
        assert_eq!(std::fs::read_dir(home.join("projects")).unwrap().count(), 0);
        assert!(!home.join("skills").exists());
        assert!(!home.join("settings.json").exists());
        assert_eq!(mode_of(&home.join(".credentials.json")), 0o600);

        // `.claude.json` minimal : onboarding seul, pas de bloc projects.
        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(staged_claude_json(sandbox_dir.path(), "run1")).unwrap(),
        )
        .unwrap();
        assert_eq!(json["hasCompletedOnboarding"], serde_json::json!(true));
        assert!(json.get("projects").is_none(), "None → objet nu");
        assert_eq!(mode_of(&staged_claude_json(sandbox_dir.path(), "run1")), 0o600);
    }

    #[test]
    fn prepare_pure_seeds_trust_when_root_given() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        write_mode(&home_dir.path().join(".claude/.credentials.json"), "{}", 0o600);
        let trusted = Path::new("/repo/root");

        prepare(
            home_dir.path(),
            sandbox_dir.path(),
            Mode::Pure,
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
        assert_eq!(entry["hasCompletedProjectOnboarding"], serde_json::json!(true));
    }

    #[test]
    fn prepare_pure_bare_object_when_no_root() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();

        prepare(home_dir.path(), sandbox_dir.path(), Mode::Pure, "run1", None).unwrap();

        let json: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(staged_claude_json(sandbox_dir.path(), "run1")).unwrap(),
        )
        .unwrap();
        // Exactement une clé : hasCompletedOnboarding.
        assert_eq!(json, serde_json::json!({ "hasCompletedOnboarding": true }));
    }

    // -- merge_back ----------------------------------------------------------

    /// Écrit un transcript de staging (projects/<ENC>/...).
    fn stage_transcript(sandbox: &Path, run_id: &str, rel: &str, content: &str) {
        let p = staged_claude_home(sandbox, run_id).join("projects").join(rel);
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
        stage_transcript(sandbox, "run1", &format!("{ENC}/notes.md"), "not a transcript");
        stage_transcript(sandbox, "run1", &format!("{ENC}/.meta.json"), "{}");

        merge_back(home, sandbox, "run1").unwrap();

        let hp = host_projects(home);
        assert_eq!(std::fs::read_to_string(hp.join(format!("{ENC}/sess.jsonl"))).unwrap(), "s\n");
        assert_eq!(
            std::fs::read_to_string(hp.join(format!("{ENC}/{UUID}/subagents/agent.jsonl"))).unwrap(),
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
        assert_eq!(std::fs::read_to_string(&host_file).unwrap(), "l1\nl2\nl3\nl4\n");

        // 3e appel sans changement → no-op (sentinelle de taille égale survit).
        std::fs::write(&host_file, "X1\nX2\nX3\nX4\n").unwrap();
        merge_back(home, sandbox, "run1").unwrap();
        assert_eq!(std::fs::read_to_string(&host_file).unwrap(), "X1\nX2\nX3\nX4\n");
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
        assert!(host_projects(home).join(format!("{ENC}/sess.jsonl")).is_file());
    }

    #[test]
    fn merge_back_noop_when_no_projects() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        // Staging pure : claude-home existe mais sans projects/.
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
        assert!(host_projects(home).join(format!("{ENC}/sess.jsonl")).is_file());
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
    fn prepare_pure_then_merge_and_teardown_roundtrip() {
        let home_dir = tempfile::tempdir().unwrap();
        let sandbox_dir = tempfile::tempdir().unwrap();
        let (home, sandbox) = (home_dir.path(), sandbox_dir.path());

        prepare(home, sandbox, Mode::Pure, "run1", None).unwrap();
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
        assert!(host_projects(home).join(format!("{ENC}/sess.jsonl")).is_file());
    }
}
