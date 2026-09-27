//! Where a sandboxed Run's staged home lives, and what it mounts: path math only
//! (#404, #432, #708). The filesystem walk that fills it is
//! [`crate::adapters::staging`].

use std::path::{Path, PathBuf};

use pdo_harness::probes::StagingSet;

use crate::domain::profile::{landing, Landing};

/// La racine de home de `claude` : servie par le mount fixe `claude-home`, jamais un
/// anchor ni un mount de home de harnais séparé (ADR-0063 §3). Bare `.claude` n'est
/// pas classé `ClaudeHome` par [`crate::domain::profile::landing`] (qui ne strippe que
/// `.claude/`), d'où ce littéral de garde.
pub const CLAUDE_HOME_REL: &str = ".claude";

/// `<sandbox_root>/<run_id>` — racine du staging d'un Run (les 2 sources de mount
/// + le `.claude.json`).
pub fn staging_dir_for_run(sandbox_root: &Path, run_id: &str) -> PathBuf {
    sandbox_root.join(run_id)
}

/// `<staging_dir>/claude-home` — le *staged Claude home* (→ `$HOME/.claude` côté
/// conteneur, monté tel quel par #406).
pub fn staged_claude_home(sandbox_root: &Path, run_id: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id).join("claude-home")
}

/// `<staging_dir>/.claude.json` — sibling de `claude-home/` (→ `$HOME/.claude.json`
/// côté conteneur, monté séparément par #406). **Caché + hors `claude-home/`** :
/// s'il vivait dans `claude-home/` il atterrirait à `$HOME/.claude/.claude.json`,
/// invisible pour Claude Code.
pub fn staged_claude_json(sandbox_root: &Path, run_id: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id).join(".claude.json")
}

/// `<staging_dir>/home` — racine des **entrées d'exception `$HOME`** (#432,
/// ADR-0031 §4). Une entrée hors `.claude` y est *copiée* (`<staging>/home/<rel>`)
/// puis bind-montée rw à `$HOME/<rel>` : **jamais** un bind direct du fichier hôte,
/// sinon un agent en `--dangerously-skip-permissions` qui bute sur
/// `unable to auto-detect email address` réécrirait le `~/.gitconfig` de
/// l'utilisateur.
pub fn staged_home_extras(sandbox_root: &Path, run_id: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id).join("home")
}

/// Où une entrée `$HOME`-relative atterrit dans le staging d'un Run — **la** vue
/// chemin du classificateur [`crate::domain::profile::landing`], partagée par le
/// profil et par les staging sets (ADR-0063) : `.claude/<rel>` →
/// `claude-home/<rel>`, `.claude.json` → le sibling, tout le reste → `home/<rel>`.
/// Un glob n'a pas de chemin unique : il dégrade en son motif littéral (aucun set
/// n'en déclare ; le profil les expanse lui-même).
pub fn staged_path(sandbox_root: &Path, run_id: &str, rel: &str) -> PathBuf {
    match landing(rel) {
        Landing::ClaudeHome { rel, .. } => staged_claude_home(sandbox_root, run_id).join(rel),
        Landing::ClaudeJson => staged_claude_json(sandbox_root, run_id),
        Landing::HomeExtra { rel } => staged_home_extras(sandbox_root, run_id).join(rel),
    }
}

/// Un bind-mount d'exception `$HOME` : `source` (dans le staging) → `target` (dans
/// le conteneur). Calculé par [`crate::adapters::staging::extra_mounts`], consommé par
/// [`crate::domain::container::ContainerSpec::extra_mounts`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedMount {
    pub source: PathBuf,
    pub target: PathBuf,
}

/// Les mounts des **racines de home des harnais first-party** (#708, ADR-0063 §3) :
/// un `-v <staging>/home/<home_root>:<host_home>/<home_root>:rw` par staging set dont
/// la racine n'est **pas** déjà servie par un mount fixe (`.claude` l'est —
/// `claude-home` —, `.pi/agent` ne l'est pas). Montées **vides** à la création du
/// conteneur, remplies par [`crate::adapters::staging::fill_staging_set`] au spawn.
///
/// Total (aucun test de disque, contrairement à [`crate::adapters::staging::extra_mounts`]) : la source est
/// créée par [`crate::adapters::staging::ensure_mount_anchors`] sur tous les chemins qui mènent au `docker
/// create`. Ordre = ordre du registre, déterministe pour le golden.
pub fn harness_home_mounts(
    sandbox_root: &Path,
    run_id: &str,
    host_home: &Path,
) -> Vec<StagedMount> {
    pdo_harness::probes::staging_sets()
        .into_iter()
        .filter(|(_, set)| set.home_root != CLAUDE_HOME_REL)
        .filter_map(|(_, set)| match landing(set.home_root) {
            Landing::HomeExtra { rel } => Some(StagedMount {
                source: staged_home_extras(sandbox_root, run_id).join(rel),
                target: host_home.join(rel),
            }),
            // `.claude.json`, or `.claude/<sub>` : servis par les mounts fixes.
            Landing::ClaudeHome { .. } | Landing::ClaudeJson => None,
        })
        .collect()
}

/// Ce qu'a produit [`crate::adapters::staging::fill_staging_set`] pour un `(Run, harnais)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagingFill {
    /// Le harnais ne déclare aucun set (ADR-0051 : valeur déclarée, dite une fois
    /// par l'appelant).
    NoSet,
    /// Le set vient d'être copié — c'est le premier spawn du Run qui résout ce
    /// harnais.
    Filled(StagingSet),
    /// Le marqueur existait : un spawn antérieur du Run a déjà posé ce set (nœud
    /// repris, second nœud du même harnais, manager puis nœud).
    AlreadyFilled(StagingSet),
}

impl StagingFill {
    /// Le set résolu, quel que soit le moment de sa copie ; `None` sans set.
    pub fn set(&self) -> Option<&StagingSet> {
        match self {
            StagingFill::NoSet => None,
            StagingFill::Filled(set) | StagingFill::AlreadyFilled(set) => Some(set),
        }
    }

    /// L'env du set, prêt pour le `docker exec` (vide sans set — `claude` n'en a pas,
    /// donc l'argv de ses nœuds reste byte-identique).
    pub fn env(&self) -> Vec<(String, String)> {
        self.set()
            .map(|set| {
                set.env
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// `<staging_dir>/sets/<harness>` — le **marqueur** « le set de ce harnais est posé
/// dans ce Run ». Écrit **après** la copie : un crash entre les deux fait recopier
/// (additif, idempotent), jamais sauter.
pub fn staging_set_marker(sandbox_root: &Path, run_id: &str, harness: &str) -> PathBuf {
    staging_dir_for_run(sandbox_root, run_id)
        .join("sets")
        .join(harness)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
