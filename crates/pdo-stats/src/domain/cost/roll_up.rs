//! « Roll up child runs » (ADR-0101 §2): Stats › Cost files
//! every descendant's cost under the Pipeline and the Project of its **root**,
//! split « Parent run » / « Child runs » › Depth k › child Pipeline › Node, and
//! recomposes the cohort by root — the whole tree enters its root's period,
//! « completed runs only » judges the root. Off, nothing here runs.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use pdo_run::domain::run_cost::tree::RunTree;
use pdo_run::domain::run_cost::CostContribution;

use super::fold::{
    add_node_rows, add_pipeline_run, add_work, add_work_to_project, node_work, CostRunRow,
};
use super::metric::CostAggregateAcc;
use super::pairs::CostEntityAcc;
use super::wire::{
    wire_cost_entity, wire_cost_pipeline, wire_cost_pipelines, wire_cost_projects, wire_periods,
    CostProjectAcc,
};
use super::{StatsCost, StatsCostDepth, StatsCostRollUp};
use crate::domain::absorption::resolver::AbsorptionResolver;

/// The ids of the two roll-up rows under a root Pipeline.
pub const ROLL_UP_PARENT_ID: &str = "parent_run";
pub const ROLL_UP_CHILDREN_ID: &str = "child_runs";

/// The child links of the whole log: which Runs are roots, and who descends
/// from whom. A Run whose parent the log no longer holds (forgotten) is a
/// root again.
pub struct RollUpCohort {
    tree: RunTree,
    linked: HashSet<String>,
}

impl RollUpCohort {
    /// `links`: each child Run, with the parent the log still holds.
    pub fn new<'a>(links: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let links: Vec<(&str, &str)> = links.into_iter().collect();
        Self {
            linked: links.iter().map(|(run, _)| run.to_string()).collect(),
            tree: RunTree::from_links(links.iter().map(|(run, parent)| (*run, Some(*parent)))),
        }
    }

    /// No parent the log still holds.
    pub fn is_root(&self, run_id: &str) -> bool {
        !self.linked.contains(run_id)
    }

    /// Every descendant of `root`, with its depth (1 = a direct child),
    /// shallowest first.
    pub fn descendants(&self, root: &str) -> Vec<(String, u32)> {
        self.tree.descendants(root)
    }
}

/// One root Run of the cohort and every one of its descendants, with its
/// depth. Each member's `bucket` and Project are its root's: the whole tree
/// follows the root.
pub struct RolledTree {
    pub root: CostRunRow,
    pub members: Vec<(u32, CostRunRow)>,
}

/// The roll-up accumulator of one root Pipeline row.
#[derive(Debug, Clone, Default)]
pub struct RollUpAcc {
    /// The root runs, as their own Pipeline row would count them.
    parent: CostEntityAcc,
    /// Every descendant, one execution per child run.
    children: CostEntityAcc,
    child_runs: BTreeSet<String>,
    depths: BTreeMap<u32, DepthAcc>,
}

#[derive(Debug, Clone, Default)]
struct DepthAcc {
    entity: CostEntityAcc,
    runs: BTreeSet<String>,
    pipelines: BTreeMap<String, CostEntityAcc>,
}

/// Count a whole tree into its root's Pipeline row: the headline is one
/// execution per tree — the root's work plus every descendant's —, the Node
/// rows stay the root's own, and the roll-up splits Parent run / Child runs ›
/// Depth k › child Pipeline.
fn add_rolled_tree(pipeline: &mut CostEntityAcc, tree: &RolledTree) {
    let root = &tree.root;
    let bucket = root.bucket.as_str();
    let root_work = node_work(&root.contributions);
    let member_work: Vec<Vec<CostContribution>> = tree
        .members
        .iter()
        .map(|(_, member)| node_work(&member.contributions))
        .collect();
    let tree_work: Vec<CostContribution> = root_work
        .iter()
        .chain(member_work.iter().flatten())
        .cloned()
        .collect();

    root.identity.adopt_name(&mut pipeline.name);
    pipeline
        .tallies
        .record_run(&root.identity, &root.run_id, &root.started_at);
    add_work(pipeline, bucket, &tree_work);
    add_node_rows(pipeline, root, bucket);

    let roll_up = pipeline.roll_up.get_or_insert_with(Default::default);
    add_pipeline_run(&mut roll_up.parent, root, bucket, &root_work);
    for ((depth, member), work) in tree.members.iter().zip(&member_work) {
        roll_up.child_runs.insert(member.run_id.clone());
        add_work(&mut roll_up.children, bucket, work);
        let level = roll_up.depths.entry(*depth).or_default();
        level.runs.insert(member.run_id.clone());
        add_work(&mut level.entity, bucket, work);
        let child_pipeline = level
            .pipelines
            .entry(member.identity.pipeline_key.clone())
            .or_default();
        add_pipeline_run(child_pipeline, member, bucket, work);
    }
}

/// The roll-up of root Pipeline row `pipeline` — `None` when none of its root
/// runs orchestrated a child run: such a row reads exactly as without roll-up.
pub fn wire_roll_up(
    pipeline: &str,
    roll_up: RollUpAcc,
    resolver: &AbsorptionResolver,
) -> Option<StatsCostRollUp> {
    if roll_up.child_runs.is_empty() {
        return None;
    }
    // The parent's Node rows sit under the root Pipeline's key, so its Node
    // absorptions apply; the row itself is no Pipeline and combines nothing.
    let mut parent = wire_cost_pipeline(pipeline.to_string(), roll_up.parent, resolver);
    parent.id = ROLL_UP_PARENT_ID.to_string();
    parent.name = "Parent run".to_string();
    parent.absorbed = Vec::new();

    let mut children_acc = roll_up.children;
    children_acc.name = "Child runs".to_string();
    let mut children = wire_cost_entity(ROLL_UP_CHILDREN_ID.to_string(), children_acc, resolver);
    children.runs = Some(roll_up.child_runs.len() as u64);

    let depths: Vec<StatsCostDepth> = roll_up
        .depths
        .into_iter()
        .map(|(depth, acc)| {
            let mut entity_acc = acc.entity;
            entity_acc.name = format!("Depth {depth}");
            let mut entity = wire_cost_entity(format!("depth:{depth}"), entity_acc, resolver);
            entity.runs = Some(acc.runs.len() as u64);
            StatsCostDepth {
                entity,
                depth,
                pipelines: wire_cost_pipelines(acc.pipelines, resolver),
            }
        })
        .collect();
    Some(StatsCostRollUp {
        parent,
        children,
        levels: depths.len() as u32,
        depths,
    })
}

/// The same response, every tree filed under its root. Total and period bars
/// count every Run of every tree in its root's period; Pipeline and Project
/// rows are the roots' (a child Pipeline keeps only the runs nobody
/// orchestrated). « By model » is not concerned: it stays `regular`'s, the
/// fold of the unchanged cohort.
pub fn fold_rolled_up_cost(
    trees: &[RolledTree],
    regular: StatsCost,
    resolver: &AbsorptionResolver,
) -> StatsCost {
    let mut total = CostAggregateAcc::default();
    let mut periods = BTreeMap::<String, CostAggregateAcc>::new();
    let mut pipelines = BTreeMap::<String, CostEntityAcc>::new();
    let mut projects = BTreeMap::<String, CostProjectAcc>::new();
    let mut active_harnesses: BTreeSet<String> = regular.harnesses.into_iter().collect();

    for tree in trees {
        let root = &tree.root;
        let runs = std::iter::once(root).chain(tree.members.iter().map(|(_, member)| member));
        let mut tree_work = Vec::new();
        for run in runs {
            total.add_run(&run.contributions);
            periods
                .entry(root.bucket.clone())
                .or_default()
                .add_run(&run.contributions);
            tree_work.extend(node_work(&run.contributions));
            active_harnesses.extend(
                run.contributions
                    .iter()
                    .filter(|c| c.executions > 0)
                    .map(|c| c.harness.clone()),
            );
        }

        let pipeline_key = &root.identity.pipeline_key;
        add_rolled_tree(pipelines.entry(pipeline_key.clone()).or_default(), tree);

        let project = projects.entry(root.project_id.clone()).or_default();
        project.name = root.project_name.clone();
        add_work_to_project(project, &root.bucket, &tree_work);
        add_rolled_tree(
            project.pipelines.entry(pipeline_key.clone()).or_default(),
            tree,
        );
    }

    StatsCost {
        harnesses: active_harnesses.into_iter().collect(),
        total: total.wire(),
        by_period: wire_periods(periods),
        by_pipeline: wire_cost_pipelines(pipelines, resolver),
        by_project: wire_cost_projects(projects, resolver),
        by_model: regular.by_model,
        resolved: regular.resolved,
    }
}

#[cfg(test)]
#[path = "roll_up_tests.rs"]
mod tests;
