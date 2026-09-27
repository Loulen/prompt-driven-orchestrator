//! The delivery of a node's skills into its worktree (#672).

use std::path::Path;

use pdo_skill::domain::selection::{ActiveSkill, SkillTier};

use crate::fakes::fixture;

fn active(id: &str, name: &str) -> ActiveSkill {
    ActiveSkill {
        id: id.into(),
        name: name.into(),
        tiers: vec![SkillTier::Instance],
    }
}

#[test]
fn the_snapshot_then_the_delivery_and_every_skip_is_reported() {
    let f = fixture();
    f.delivery.absent.lock().unwrap().push("s3".into());
    f.delivery.versioned.lock().unwrap().push("x".into());
    let skills = [active("s1", "tdd"), active("s2", "x"), active("s3", "gone")];
    let skipped = f
        .skills
        .deliver(Path::new("/wt"), Path::new("/repo"), "r1", &skills)
        .unwrap();
    let reasons: Vec<_> = skipped
        .iter()
        .map(|s| (s.id.as_str(), s.reason.as_str()))
        .collect();
    assert_eq!(
        reasons,
        vec![
            ("s3", "skill content is absent from the bank"),
            ("s2", "versioned")
        ]
    );
    let snapshots = f.delivery.snapshots.lock().unwrap().clone();
    assert_eq!(snapshots[0].1, "r1");
    let deliveries = f.delivery.deliveries.lock().unwrap().clone();
    assert_eq!(deliveries[0].0, Path::new("/wt"));
    assert_eq!(deliveries[0].1, Path::new("/repo/.pdo/runs/r1/skills"));
    // A transition of the scheduler: never an Autorisation d'opération.
    assert!(f.host.asked().is_empty());
}

#[test]
fn withdraw_takes_the_runs_exclusions_back() {
    let f = fixture();
    f.skills.withdraw(Path::new("/repo"), "r1").unwrap();
    assert_eq!(
        *f.delivery.withdrawn.lock().unwrap(),
        vec!["r1".to_string()]
    );
}
