//! The copies of the staging: the profile's entries (phase 1), and the
//! symlink-aware tree walk and the transcript merge they share with the staging sets.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use tracing::warn;

use crate::domain::profile::{landing, Landing};

/// Phase 1 : une passe unique sur la liste gelée, chaque entrée routée par le
/// classificateur **unique** [`crate::domain::profile::landing`] — le même que
/// [`super::extra_mounts`] consulte, donc la vue *copie* et la vue *mount* ne peuvent pas
/// diverger.
///
/// N'inclut **aucune** entrée d'un staging set : ceux-ci tournent juste après, quelle
/// que soit la liste. Une liste vide (`minimal`) est un no-op exact — la traduction
/// littérale de l'ancien bras `Mode::Minimal => {}`.
///
/// Politique manquant-sur-l'hôte : `warn!` + skip, pour les entrées du **défaut**
/// comme pour les **extras**. L'échec dur a été écarté (plan #432 D5) : il ferait
/// dépendre la politique de *qui a tapé le chemin* plutôt que de ce dont le conteneur
/// a besoin — sur une instance à Triggers horaires, désinstaller `gh` tuerait chaque
/// tir jusqu'à édition d'un profil. Et la règle M1 d'[`super::extra_mounts`] supprime le
/// danger réel (le répertoire root-owned), qui était la vraie justification du
/// fail-fast.
pub(super) fn materialise_entries(
    home_root: &Path,
    src: &Path,
    home: &Path,
    staged_json: &Path,
    staging: &Path,
    entries: &[String],
) -> Result<()> {
    // `copy_root` du bloc `.claude` : la racine canonique `~/.claude`. Tout symlink
    // dont la cible sort de CET arbre est copié déréférencé (sa cible réelle n'est ni
    // copiée ni montée → danglerait, #409). Résolue une fois, partagée par toutes les
    // entrées `ClaudeHome` — c'est ce qui garde un lien `skills/x → ../agents/y` un
    // lien.
    let claude_copy_root = std::fs::canonicalize(src).unwrap_or_else(|_| src.to_path_buf());

    for entry in entries {
        match landing(entry) {
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
                stage_path(&from, &to, &claude_copy_root, &[], entry)?;
            }
            // `.claude.json` sibling, verbatim (mode préservé). La confiance et
            // l'onboarding y sont mergés ensuite par le fixup `ClaudeJsonBaseline` (G4).
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
                stage_path(&from, &to, &copy_root, &[], entry)?;
            }
        }
    }
    Ok(())
}

/// Stage une entrée dont on ne sait pas a priori si c'est un fichier ou un dossier.
/// Dossier → walk préservant symlinks + bits exécutables ([`copy_tree_preserving`],
/// best-effort par entrée, `skip` = sous-chemins hôte absolus à ne pas copier) ;
/// fichier → [`std::fs::copy`] (mode préservé, dont 0600) ; absent → `warn!` + skip.
fn stage_path(
    from: &Path,
    to: &Path,
    copy_root: &Path,
    skip: &[PathBuf],
    entry: &str,
) -> Result<()> {
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
            copy_tree_preserving(from, to, copy_root, skip, 0);
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

/// Copie `src` → `dst` s'il existe (fichier de l'allowlist absent = no-op).
/// [`std::fs::copy`] préserve le mode sous Unix (dont 0600 des credentials), même
/// en écrasant une destination existante.
pub(super) fn copy_file_if_present(src: &Path, dst: &Path) -> Result<()> {
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
/// jugée « échappante ». `skip` = chemins hôte absolus **non copiés** (exclusions et
/// puits de transcripts d'un staging set, ADR-0063 ; vide pour le profil). `depth` =
/// profondeur courante ([`MAX_COPY_DEPTH`]).
///
/// N.B. : ne PAS réutiliser `copy_dir_all` de `lib.rs` — il n'est pas
/// symlink-aware (`std::fs::copy` déréférence).
pub(super) fn copy_tree_preserving(
    src: &Path,
    dst: &Path,
    copy_root: &Path,
    skip: &[PathBuf],
    depth: u32,
) {
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
        if skip.contains(&from) {
            continue;
        }
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
            copy_tree_preserving(&from, &to, copy_root, skip, depth + 1);
        } else if ft.is_file() {
            if let Err(e) = std::fs::copy(&from, &to) {
                warn!(
                    "sandbox copy: skip file {} -> {} ({e:#})",
                    from.display(),
                    to.display()
                );
            }
        }
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
        let _ = std::fs::remove_file(to);
        if let Err(e) = std::os::unix::fs::symlink(&link_target, to) {
            warn!(
                "sandbox copy: skip symlink {} -> {} ({e:#})",
                to.display(),
                link_target.display()
            );
        }
    } else if canonical.is_dir() {
        copy_tree_preserving(&canonical, to, &canonical, &[], depth + 1);
    } else if canonical.is_file() {
        let _ = std::fs::remove_file(to);
        if let Err(e) = std::fs::copy(&canonical, to) {
            warn!(
                "sandbox copy: skip deref file {} -> {} ({e:#})",
                canonical.display(),
                to.display()
            );
        }
    }
}

/// Recopie récursivement les `*.jsonl` de `src_dir` vers `dest_dir`, copy-if-
/// absent-or-larger, atomiquement. Miroir de
/// the cost fold's `collect_jsonl_recursive` (même prédicat `is_dir` +
/// extension `jsonl`) pour que le copy-set égale le read-set du coût.
/// Best-effort : tout échec est avalé (jamais de propagation vers l'appelant).
pub(super) fn copy_jsonl_tree(src_dir: &Path, dest_dir: &Path) {
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
