//! Provisioning rules: which untracked files of the source repository a Run's
//! worktree receives, and how (copy, hardlink, symlink), declared per scope.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ProvisioningScope {
    Instance,
    Project,
    Run,
    IsolatedNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum ProvisioningMode {
    Copy,
    Hardlink,
    Symlink,
}

impl std::fmt::Display for ProvisioningMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Copy => write!(f, "copy"),
            Self::Hardlink => write!(f, "hardlink"),
            Self::Symlink => write!(f, "symlink"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ProvisioningRules {
    #[serde(default)]
    pub copy: Vec<String>,
    #[serde(default)]
    pub hardlink: Vec<String>,
    #[serde(default)]
    pub symlink: Vec<String>,
}

impl ProvisioningRules {
    pub(crate) fn patterns(&self) -> impl Iterator<Item = (ProvisioningMode, &str)> {
        self.copy
            .iter()
            .map(|p| (ProvisioningMode::Copy, p.as_str()))
            .chain(
                self.hardlink
                    .iter()
                    .map(|p| (ProvisioningMode::Hardlink, p.as_str())),
            )
            .chain(
                self.symlink
                    .iter()
                    .map(|p| (ProvisioningMode::Symlink, p.as_str())),
            )
    }

    pub fn is_empty(&self) -> bool {
        self.copy.is_empty() && self.hardlink.is_empty() && self.symlink.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "ScopedProvisioningRules")]
pub struct ScopedRules {
    pub scope: ProvisioningScope,
    #[serde(default)]
    pub rules: ProvisioningRules,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct ProvisioningEntry {
    pub relative_path: String,
    pub mode: ProvisioningMode,
    pub origin_scope: ProvisioningScope,
    pub pattern: String,
    pub provided_by_git: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ExcludedPathPreview {
    pub relative_path: String,
    pub excluded_by_scope: ProvisioningScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, rename = "ProvisioningRulePreview")]
pub struct RulePreview {
    pub scope: ProvisioningScope,
    pub mode: ProvisioningMode,
    pub pattern: String,
    pub paths: Vec<String>,
    pub excluded_paths: Vec<ExcludedPathPreview>,
    pub unmatched: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ModeConflict {
    pub scope: ProvisioningScope,
    pub relative_path: String,
    pub modes: Vec<ProvisioningMode>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct ProvisioningPlan {
    pub entries: Vec<ProvisioningEntry>,
    pub rules: Vec<RulePreview>,
    pub conflicts: Vec<ModeConflict>,
}

/// The rules a preview at `scope` resolves: every inherited level up to and
/// including `scope`, with `edited` standing in for that scope's own rules (the
/// editor previews what it is about to save, not what is stored).
pub fn preview_levels(
    mut inherited: Vec<ScopedRules>,
    scope: ProvisioningScope,
    edited: ProvisioningRules,
) -> Vec<ScopedRules> {
    inherited.retain(|candidate| candidate.scope <= scope);
    if let Some(current) = inherited.iter_mut().find(|rules| rules.scope == scope) {
        current.rules = edited;
    } else {
        inherited.push(ScopedRules {
            scope,
            rules: edited,
        });
        inherited.sort_by_key(|rules| rules.scope);
    }
    inherited
}

#[cfg(test)]
mod tests {
    use super::*;

    fn level(scope: ProvisioningScope, copy: &[&str]) -> ScopedRules {
        ScopedRules {
            scope,
            rules: ProvisioningRules {
                copy: copy.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            },
        }
    }

    #[test]
    fn a_preview_drops_the_finer_levels_and_replaces_its_own() {
        let inherited = vec![
            level(ProvisioningScope::Instance, &[".env"]),
            level(ProvisioningScope::Project, &["stored"]),
            level(ProvisioningScope::Run, &["run"]),
        ];
        let edited = ProvisioningRules {
            copy: vec!["edited".into()],
            ..Default::default()
        };
        let levels = preview_levels(inherited, ProvisioningScope::Project, edited);
        assert_eq!(
            levels,
            vec![
                level(ProvisioningScope::Instance, &[".env"]),
                level(ProvisioningScope::Project, &["edited"]),
            ]
        );
    }

    #[test]
    fn a_missing_level_is_inserted_in_scope_order() {
        let inherited = vec![
            level(ProvisioningScope::Instance, &["a"]),
            level(ProvisioningScope::Run, &["r"]),
        ];
        let levels = preview_levels(
            inherited,
            ProvisioningScope::Run,
            ProvisioningRules::default(),
        );
        assert_eq!(levels.len(), 2);
        let levels = preview_levels(
            vec![level(ProvisioningScope::Run, &["r"])],
            ProvisioningScope::Project,
            ProvisioningRules::default(),
        );
        assert_eq!(levels, vec![level(ProvisioningScope::Project, &[])]);
    }

    #[test]
    fn empty_rules_are_empty() {
        assert!(ProvisioningRules::default().is_empty());
        assert!(!level(ProvisioningScope::Run, &["x"]).rules.is_empty());
    }
}
