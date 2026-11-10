//! Applying a harness's **staging set** (ADR-0063) to a Run's staging: its entries,
//! its empty transcript sinks, and its autonomy fixups.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use pdo_harness::probes::{AutonomyFixup, StagingSet};
use tracing::{info, warn};

use super::copy::copy_tree_preserving;
use crate::domain::staging::{staged_claude_home, staged_path};

/// Le `settings.json` du home stagé — porteur de la garantie G3
/// ([`pdo_harness::probes::AutonomyFixup::ClaudeBypassPermissions`]).
pub(super) const SETTINGS_FILE: &str = "settings.json";

/// Clé top-level qui désarme le prompt de confirmation du
/// `--dangerously-skip-permissions`. Garantie G3 : un agent non surveillé s'y
/// bloque. Résolue par un **OU monotone** sur les tiers de settings, donc un
/// `true` dans le `settings.json` stagé suffit — et un `false` hôte doit être
/// **écrasé** (d'où `insert` et non `entry().or_insert()`).
pub(super) const BYPASS_PERMISSIONS_KEY: &str = "skipDangerousModePermissionPrompt";

/// Applique **un staging set** (ADR-0063) au staging d'un Run : les garanties que
/// `prepare` tient quel que soit le profil. Écrit exclusivement dans le staging ; ne
/// touche jamais l'hôte. Trois gestes, dans cet ordre, sur des chemins disjoints :
///
/// 1. **Entrées** — chaque `rel` copié de `$HOME/<rel>` vers [`staged_path`]. Un
///    fichier via [`std::fs::copy`] (mode préservé, dont 0600 des credentials) ; un
///    répertoire via [`copy_tree_preserving`] en sautant `excludes` et les puits de
///    transcripts. Absent hôte → no-op, avec la ligne `info!` de l'entrée si elle en
///    porte une (un `warn!` par Run entraînerait à ignorer les warnings). Présent
///    mais copie en échec → erreur **dure** : c'est une surprise de conformité, pas
///    un détail.
/// 2. **Puits de transcripts** — créés **vides** (jamais copiés : ni le puits hôte ni
///    le sous-dir encodé n'existent pour un Run frais ; les copier casserait
///    l'idempotence du merge-back et le calcul de coût).
/// 3. **Autonomy fixups** — les écritures qui désarment un dialogue bloquant
///    ([`AutonomyFixup`]), chacune un merge non destructif dans ce que la phase 1 a
///    posé, ou une synthèse quand elle n'a rien posé.
///
/// Pour `claude`, c'est ADR-0031 §1 mot pour mot : G1 credentials, G2 managed
/// settings de l'org (entrées) ; G5 `projects/` vide (puits) ; G3 bypass permissions,
/// G4 baseline `.claude.json` (fixups). L'`env` du set n'est pas posé ici : il part
/// sur le `docker exec` de la session ([`crate::domain::staging::StagingFill::env`]) ; `claude` n'en a pas.
///
/// Politique d'écriture : **fail-fast**. Une garantie intenable doit échouer au
/// spawn, pas produire une session qui pend sur un dialogue sans personne devant. Ni le best-effort de
/// [`copy_tree_preserving`] (arbre de 1 Go volatil) ni l'avalage de
/// [`super::copy::copy_jsonl_tree`] (transition terminale, ADR-0023) ne s'appliquent ici.
pub(super) fn apply_staging_set(
    home_root: &Path,
    sandbox_root: &Path,
    run_id: &str,
    harness: &str,
    set: &StagingSet,
    trusted_root: Option<&Path>,
) -> Result<()> {
    // 1 — entrées. `skip` = exclusions + puits de transcripts, en chemins hôte absolus
    // (c'est ce que le walk compare).
    let skip: Vec<PathBuf> = set
        .excludes
        .iter()
        .chain(set.transcripts.iter())
        .map(|rel| home_root.join(rel))
        .collect();
    for entry in set.entries {
        let from = home_root.join(entry.rel);
        let to = staged_path(sandbox_root, run_id, entry.rel);
        if !from.exists() {
            if let Some(note) = entry.absent_note {
                // `info!` et non `debug!` : le filtre par défaut du daemon est
                // `info` (`crates/pdo/src/entry.rs`).
                info!(
                    "sandbox staging: no {} at {} — {note}",
                    entry.rel,
                    from.display()
                );
            }
            continue;
        }
        if from.is_dir() {
            let copy_root = std::fs::canonicalize(&from).unwrap_or_else(|_| from.clone());
            copy_tree_preserving(&from, &to, &copy_root, &skip, 0);
        } else {
            if let Some(parent) = to.parent() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("create parent {}", parent.display()))?;
            }
            std::fs::copy(&from, &to).with_context(|| {
                format!(
                    "stage {harness} set entry {} -> {}",
                    from.display(),
                    to.display()
                )
            })?;
        }
    }

    // 2 — puits de transcripts, créés VIDES.
    for rel in set.transcripts {
        let sink = staged_path(sandbox_root, run_id, rel);
        std::fs::create_dir_all(&sink).with_context(|| {
            format!(
                "create staged {harness} transcripts sink {}",
                sink.display()
            )
        })?;
    }

    // 3 — autonomy fixups.
    for fixup in set.fixups {
        match fixup {
            // G3 — bypass permissions. `insert` (pas `entry().or_insert()`) : un
            // `false` hôte doit être écrasé, sinon l'agent se bloque sur le prompt.
            // Pas de `chmod` (ce n'est pas un secret ; `fs::write` tronque en place et
            // préserve donc le mode hôte en `full`) et pas de tmp+rename : aucun
            // lecteur concurrent n'existe avant le conteneur — contrairement à
            // `merge_back`/`atomic_copy_into`.
            AutonomyFixup::ClaudeBypassPermissions => {
                let settings = staged_claude_home(sandbox_root, run_id).join(SETTINGS_FILE);
                edit_json_object(&settings, "staged settings.json", |obj| {
                    obj.insert(BYPASS_PERMISSIONS_KEY.to_string(), serde_json::json!(true));
                })?;
            }
            // G4 — baseline du `.claude.json` stagé (onboarding + confiance).
            AutonomyFixup::ClaudeJsonBaseline => {
                let staged_json = staged_path(sandbox_root, run_id, ".claude.json");
                ensure_claude_json_baseline(&staged_json, trusted_root)?;
            }
        }
    }

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

/// Garantie **G4** du set de `claude` ([`AutonomyFixup::ClaudeJsonBaseline`]) : le `.claude.json` stagé porte
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
pub(super) fn set_mode_0600(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .with_context(|| format!("chmod 0600 {}", path.display()))
}
