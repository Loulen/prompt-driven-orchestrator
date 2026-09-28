//! The repositories a fire refreshes before its cut (#804).

use pdo_run::application::create_run::TargetRepoInput;

/// Every repository a Trigger fire is about to cut from, **primary first** (#804).
///
/// Takes the Trigger's two stored fields verbatim (the secondary list is raw JSON
/// TEXT, see [`crate::domain::trigger::Trigger::target_repos`]) and mirrors the very
/// normalisation the Run's create chokepoint applies: the scalar is the primary when it is
/// set, else row 0 of the list. Blanks and duplicates fall out — a repo named twice
/// is one repo to fetch, and the ORDER is what makes "primary first" decidable by
/// the caller, which reports only the primary's failure.
///
/// Pure, and separate from the fetch, so the list rule is testable without git: a
/// malformed `target_repos` deferring to a mono-repo fire is the same rule the fire
/// itself applies, and the two must not drift.
pub fn fetch_targets(target_repo: Option<&str>, target_repos_json: Option<&str>) -> Vec<String> {
    let secondaries = target_repos_json
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .and_then(|s| serde_json::from_str::<Vec<TargetRepoInput>>(s).ok())
        .unwrap_or_default()
        .into_iter()
        .map(|r| r.repo);

    let mut targets: Vec<String> = Vec::new();
    for candidate in target_repo
        .map(str::to_string)
        .into_iter()
        .chain(secondaries)
    {
        let path = candidate.trim().to_string();
        if !path.is_empty() && !targets.contains(&path) {
            targets.push(path);
        }
    }
    targets
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trigger_fetch_targets_leads_with_the_primary_and_fetches_a_repo_once() {
        let targets = fetch_targets(
            Some("/repos/primary"),
            Some(r#"[{"repo":"/repos/primary"},{"repo":"/repos/lib"}]"#),
        );
        assert_eq!(
            targets,
            vec!["/repos/primary".to_string(), "/repos/lib".to_string()],
            "the primary leads (its failure is the one the Run shows) and the row-0 \
             echo of it is not a second repo to fetch"
        );
    }

    #[test]
    fn trigger_fetch_targets_takes_row_zero_as_the_primary_when_the_scalar_is_blank() {
        // The multi-repo modal carries the primary in row 0; the create chokepoint
        // normalises exactly this way, and the fetch order must agree with it.
        let targets = fetch_targets(
            Some("   "),
            Some(r#"[{"repo":"/repos/a"},{"repo":"/repos/b"}]"#),
        );
        assert_eq!(
            targets,
            vec!["/repos/a".to_string(), "/repos/b".to_string()]
        );
    }

    #[test]
    fn trigger_fetch_targets_defers_to_a_mono_repo_fire_on_an_unreadable_list() {
        // Same rule the fire itself applies to `target_repos`: blank, absent or
        // malformed all mean "mono-repo", never "no repo at all".
        for stored in [None, Some(""), Some("not json")] {
            assert_eq!(
                fetch_targets(Some("/repos/only"), stored),
                vec!["/repos/only".to_string()],
                "stored list {stored:?} must leave the primary standing"
            );
        }
    }
}
