//! The roll-up fold, over hand-built trees: the root's Pipeline takes the
//! whole tree, a child Pipeline keeps only the runs nobody orchestrated,
//! every member lands in its root's period and Project, « By model » stays.

use pdo_run::domain::run_cost::{CostContribution, CostScope};
use pdo_run::domain::state::CostForm;

use super::*;
use crate::domain::cost::fold::fold_harness_cost;

fn contribution(scope: CostScope, node: Option<&str>, usd: f64) -> CostContribution {
    CostContribution {
        harness: "claude".to_string(),
        scope,
        node_id: node.map(str::to_string),
        executions: 1,
        readable_executions: 1,
        usd: Some(usd),
        form: Some(CostForm::Derived),
        reported_in_usd: false,
        partial: false,
        unpriced_models: Vec::new(),
        unavailable_reasons: Vec::new(),
        model_slices: Vec::new(),
        started_event: None,
    }
}

fn row(run_id: &str, pipeline: &str, bucket: &str, project: &str, usd: f64) -> CostRunRow {
    CostRunRow {
        run_id: run_id.to_string(),
        started_at: format!("{bucket}T09:00:00Z"),
        bucket: bucket.to_string(),
        identity: AbsorptionResolver::default().run_identity(&serde_json::json!({
            "pipeline_id": pipeline,
            "pipeline_name": pipeline,
            "node_defs": [{"id": "worker", "name": "Worker", "node_type": "agent"}],
        })),
        project_id: project.to_string(),
        project_name: project.to_string(),
        contributions: vec![
            contribution(CostScope::Node, Some("worker"), usd),
            // Overhead: in the totals, never in a Pipeline's headline.
            contribution(CostScope::Infrastructure, None, 0.01),
        ],
    }
}

/// The child follows its root: the root's bucket and Project.
fn member(run_id: &str, pipeline: &str, root: &CostRunRow, usd: f64) -> CostRunRow {
    row(run_id, pipeline, &root.bucket, &root.project_id, usd)
}

fn approx(value: Option<f64>, expected: f64) -> bool {
    value.is_some_and(|v| (v - expected).abs() < 1e-9)
}

/// orchestrate ($3) ─ implement ($0.30, started a month later, another
/// Project) ─ review ($0.03); and a lone review ($0.50) nobody orchestrated.
fn world() -> (Vec<CostRunRow>, Vec<RolledTree>) {
    let root = row("root", "orchestrate", "2026-07", "prj-a", 3.0);
    let child = row("child", "implement", "2026-08", "prj-b", 0.3);
    let grandchild = row("grand", "review", "2026-08", "prj-b", 0.03);
    let lone = row("lone", "review", "2026-07", "prj-a", 0.5);
    let regular = vec![
        root.clone(),
        child.clone(),
        grandchild.clone(),
        lone.clone(),
    ];
    let trees = vec![
        RolledTree {
            members: vec![
                (1, member("child", "implement", &root, 0.3)),
                (2, member("grand", "review", &root, 0.03)),
            ],
            root,
        },
        RolledTree {
            root: lone,
            members: Vec::new(),
        },
    ];
    (regular, trees)
}

#[test]
fn a_tree_is_filed_under_its_roots_pipeline_split_by_depth() {
    let (regular, trees) = world();
    let resolver = AbsorptionResolver::default();
    let plain = fold_harness_cost(&regular, Vec::new(), &resolver);
    let rolled = fold_rolled_up_cost(&trees, plain.clone(), &resolver);

    let ids: Vec<&str> = rolled.by_pipeline.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(
        ids,
        ["orchestrate", "review"],
        "implement was all orchestrated"
    );
    let root = &rolled.by_pipeline[0];
    assert!(approx(root.aggregate.usd, 3.33), "{:?}", root.aggregate.usd);
    assert_eq!(root.runs, Some(1), "one root run");
    let roll_up = root.roll_up.as_ref().expect("a roll-up on the root row");
    assert_eq!(roll_up.parent.id, ROLL_UP_PARENT_ID);
    assert_eq!(roll_up.parent.name, "Parent run");
    assert!(approx(roll_up.parent.aggregate.usd, 3.0));
    assert!(
        roll_up
            .parent
            .nodes
            .iter()
            .any(|n| n.name == "Infrastructure"),
        "the root's own Node rows, overhead included"
    );
    assert_eq!(roll_up.children.id, ROLL_UP_CHILDREN_ID);
    assert_eq!(roll_up.children.runs, Some(2));
    assert!(approx(roll_up.children.aggregate.usd, 0.33));
    assert_eq!(roll_up.levels, 2);
    let depth_1 = &roll_up.depths[0];
    assert_eq!(
        (depth_1.depth, depth_1.entity.name.as_str()),
        (1, "Depth 1")
    );
    assert_eq!(depth_1.entity.runs, Some(1));
    assert_eq!(depth_1.pipelines[0].id, "implement");
    assert!(approx(depth_1.pipelines[0].aggregate.usd, 0.3));
    assert_eq!(depth_1.pipelines[0].nodes[0].name, "Worker");
    assert!(approx(roll_up.depths[1].entity.aggregate.usd, 0.03));
    assert_eq!(roll_up.depths[1].pipelines[0].id, "review");

    // A child Pipeline keeps only the runs nobody orchestrated.
    let review = &rolled.by_pipeline[1];
    assert!(approx(review.aggregate.usd, 0.5));
    assert_eq!(review.runs, Some(1));
    assert!(
        review.roll_up.is_none(),
        "a lone root reads as without roll-up"
    );

    // The total keeps every Run, overhead included; the period bars put the
    // whole tree in its root's period.
    assert!(approx(rolled.total.usd, plain.total.usd.unwrap()));
    assert_eq!(rolled.by_period.len(), 1);
    assert_eq!(rolled.by_period[0].bucket, "2026-07");
    assert!(approx(
        rolled.by_period[0].aggregate.usd,
        3.0 + 0.3 + 0.03 + 0.5 + 0.04
    ));

    // By project: the child follows its root's Project.
    assert_eq!(rolled.by_project.len(), 1);
    let project = &rolled.by_project[0];
    assert_eq!(project.entity.id, "prj-a");
    assert!(approx(project.entity.aggregate.usd, 3.83));
    let in_project = &project.pipelines[0];
    assert_eq!(in_project.id, "orchestrate");
    assert_eq!(in_project.roll_up.as_ref().unwrap().children.runs, Some(2));

    // « By model » is not concerned.
    assert_eq!(rolled.by_model, plain.by_model);
}

#[test]
fn off_the_roll_up_no_row_carries_one() {
    let (regular, _) = world();
    let plain = fold_harness_cost(&regular, Vec::new(), &AbsorptionResolver::default());
    assert!(plain.by_pipeline.iter().all(|p| p.roll_up.is_none()));
    let json = serde_json::to_value(&plain).unwrap();
    assert!(
        !json.to_string().contains("roll_up"),
        "the response without roll-up says nothing of it"
    );
}

#[test]
fn the_cohort_reads_roots_and_descendants_from_the_links() {
    let cohort = RollUpCohort::new([("child", "root"), ("grand", "child"), ("other", "root")]);
    assert!(cohort.is_root("root"));
    assert!(cohort.is_root("lone"), "a Run without link is a root");
    assert!(!cohort.is_root("grand"));
    assert_eq!(
        cohort.descendants("root"),
        vec![
            ("child".to_string(), 1),
            ("other".to_string(), 1),
            ("grand".to_string(), 2)
        ]
    );
    assert!(cohort.descendants("lone").is_empty());
}
