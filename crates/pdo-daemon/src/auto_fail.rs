//! Resolve a node's effective `auto_fail` policy — **pure** (ADR-0049, ADR-0015).
//!
//! `auto_fail` is the one opt-in that lets an **agent's `pdo fail`** terminalise
//! a run directly to `Failed`. Decked (the default), an agent `pdo fail` parks
//! the run `AwaitingUser` so a human confirms the failure; checked, it fails the
//! run straight away. It concerns **only** the agent `pdo fail` — every runtime
//! give-up (session death, boot recovery, spawn-abort, output-validation, merge
//! conflict, `unrouted`, run-level stall) parks regardless.
//!
//! It is resolved on the harness axis' precedence shape, coarsest last:
//! `node → Run → Projet → instance (global)`. The finest tier that states a
//! preference wins; the instance tier is a plain `bool` floor (default `false`),
//! not an `Option`, because there is always a global answer (`stored → env →
//! false`, ADR-0015). Every finer tier is `Option<bool>`: `None` = "this tier
//! states no preference, fall through".
//!
//! Pure by contract: a set of tier values in, a `bool` out — no `$HOME`, no
//! disk, no clock — so the whole precedence matrix is unit-tested without a
//! fixture (the discipline `harness_resolver` and `run_cost` pay).

/// The `auto_fail` preference named at each tier, coarsest last. `None` on a
/// finer tier = "states no preference"; the instance tier is a resolved `bool`
/// floor, never absent.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AutoFailTiers {
    /// The node's `auto_fail:` (pipeline YAML). Finest tier.
    pub node: Option<bool>,
    /// Frozen at Run creation from the `RunStarted` payload.
    pub run: Option<bool>,
    pub project: Option<bool>,
    /// The instance default (`stored → env → false`). The floor — always a
    /// resolved boolean.
    pub instance: bool,
}

/// Resolve the effective `auto_fail`: the finest tier that states a preference
/// wins, with the instance boolean as the floor.
pub(crate) fn resolve_auto_fail(tiers: &AutoFailTiers) -> bool {
    tiers
        .node
        .or(tiers.run)
        .or(tiers.project)
        .unwrap_or(tiers.instance)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_is_the_instance_bool_when_no_finer_tier_states_one() {
        assert!(!resolve_auto_fail(&AutoFailTiers {
            instance: false,
            ..Default::default()
        }));
        assert!(resolve_auto_fail(&AutoFailTiers {
            instance: true,
            ..Default::default()
        }));
    }

    #[test]
    fn project_beats_instance() {
        assert!(resolve_auto_fail(&AutoFailTiers {
            project: Some(true),
            instance: false,
            ..Default::default()
        }));
        assert!(!resolve_auto_fail(&AutoFailTiers {
            project: Some(false),
            instance: true,
            ..Default::default()
        }));
    }

    #[test]
    fn run_beats_project_and_instance() {
        assert!(resolve_auto_fail(&AutoFailTiers {
            run: Some(true),
            project: Some(false),
            instance: false,
            ..Default::default()
        }));
    }

    #[test]
    fn node_beats_every_coarser_tier() {
        assert!(resolve_auto_fail(&AutoFailTiers {
            node: Some(true),
            run: Some(false),
            project: Some(false),
            instance: false,
        }));
        assert!(!resolve_auto_fail(&AutoFailTiers {
            node: Some(false),
            run: Some(true),
            project: Some(true),
            instance: true,
        }));
    }
}
