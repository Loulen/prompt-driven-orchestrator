//! Path coherence of a Pipeline (ADR-0081, CONTEXT.md « Pipeline non lançable »).
//!
//! A Pipeline is **launchable** iff, following **every** edge (conditional `when`/
//! `else` edges and loop re-entries count as ordinary edges — coherence is
//! static, predicates stay a runtime matter, ADR-0002):
//!
//! 1. `End` is reachable from `Start`;
//! 2. no **Cul-de-sac**: every node other than `End` leads to `End`;
//! 3. no **Nœud orphelin**: every node other than `Start` is reachable from `Start`.
//!
//! `Start` is thereby the single entry point of a Pipeline. A pure function: the
//! daemon is the only authority (run creation refuses, triggers turn dangling,
//! the pipeline endpoints report), the editor's TypeScript check is a mirror held
//! to the same fixture corpus (`tests/fixtures/path-coherence/`).
//!
//! Edges naming a node that does not exist are ignored here: they are dangling
//! references, reported (and refused) by `pipeline::dangling_edge_references`.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::Serialize;

use crate::pipeline::{NodeType, PipelineDef};

/// The three defects that make a Pipeline non-launchable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PathDefectKind {
    /// `End` cannot be reached from `Start`. Names no node of its own.
    NoPath,
    /// A node (other than `End`) from which no path leads to `End`.
    DeadEnd,
    /// A node (other than `Start`) that no path from `Start` reaches.
    Orphan,
}

/// One path-coherence defect. `node_id`/`node_name` name the culprit node — the
/// structured handle the editor's « Show » link and node marks key on (#1022);
/// `None` only for [`PathDefectKind::NoPath`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct PathDefect {
    pub kind: PathDefectKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_name: Option<String>,
}

impl PathDefect {
    /// The exact user-facing sentence (grilling Q18) — one string, shared by the
    /// launch refusal, the pipeline diagnostics and the editor's banner.
    pub(crate) fn message(&self) -> String {
        let name = self.node_name.as_deref().unwrap_or("");
        match self.kind {
            PathDefectKind::NoPath => "Not launchable: no path from Start to End.".to_string(),
            PathDefectKind::DeadEnd => {
                format!("Not launchable: dead end — node \"{name}\" never leads to End.")
            }
            PathDefectKind::Orphan => {
                format!(
                    "Not launchable: orphan node — node \"{name}\" is not reachable from Start."
                )
            }
        }
    }
}

/// The wire shape of a defect: the structured defect plus its message, so a
/// client never re-derives the sentence (additive to the flat `diagnostics`).
#[derive(Debug, Clone, Serialize)]
pub(crate) struct PathDefectView {
    #[serde(flatten)]
    pub defect: PathDefect,
    pub message: String,
}

pub(crate) fn defect_views(defects: &[PathDefect]) -> Vec<PathDefectView> {
    defects
        .iter()
        .map(|defect| PathDefectView {
            defect: defect.clone(),
            message: defect.message(),
        })
        .collect()
}

/// Every path-coherence defect of `pipeline`, in a stable order: the « no path »
/// defect first, then every Cul-de-sac, then every Nœud orphelin — each group in
/// the pipeline's node order. Empty ⇔ the Pipeline is launchable.
///
/// A pipeline without exactly one `Start` and one `End` does not parse
/// (`parse_pipeline`), so it never reaches here; should it anyway, the missing
/// marker reads as « no path ».
pub(crate) fn path_defects(pipeline: &PipelineDef) -> Vec<PathDefect> {
    let start = pipeline
        .nodes
        .iter()
        .find(|n| n.node_type == NodeType::Start);
    let end = pipeline.nodes.iter().find(|n| n.node_type == NodeType::End);

    let node_ids: HashSet<&str> = pipeline.nodes.iter().map(|n| n.id.as_str()).collect();
    let mut forward: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut backward: HashMap<&str, Vec<&str>> = HashMap::new();
    for edge in &pipeline.edges {
        let (source, target) = (edge.source.node.as_str(), edge.target.node.as_str());
        if !node_ids.contains(source) || !node_ids.contains(target) {
            continue;
        }
        forward.entry(source).or_default().push(target);
        backward.entry(target).or_default().push(source);
    }

    let from_start = start
        .map(|s| reachable(s.id.as_str(), &forward))
        .unwrap_or_default();
    let to_end = end
        .map(|e| reachable(e.id.as_str(), &backward))
        .unwrap_or_default();

    let mut defects = Vec::new();
    let end_reached = end.is_some_and(|e| from_start.contains(e.id.as_str()));
    if !end_reached {
        defects.push(PathDefect {
            kind: PathDefectKind::NoPath,
            node_id: None,
            node_name: None,
        });
    }

    let culprit = |kind, node: &crate::pipeline::NodeDef| PathDefect {
        kind,
        node_id: Some(node.id.clone()),
        node_name: Some(display_name(node)),
    };
    for node in &pipeline.nodes {
        if matches!(node.node_type, NodeType::Start | NodeType::End) {
            continue;
        }
        if !to_end.contains(node.id.as_str()) {
            defects.push(culprit(PathDefectKind::DeadEnd, node));
        }
    }
    for node in &pipeline.nodes {
        if matches!(node.node_type, NodeType::Start | NodeType::End) {
            continue;
        }
        if !from_start.contains(node.id.as_str()) {
            defects.push(culprit(PathDefectKind::Orphan, node));
        }
    }
    defects
}

/// The launch refusal for a non-launchable pipeline: every defect's sentence,
/// space-joined. `None` when the pipeline is launchable.
pub(crate) fn launch_refusal(defects: &[PathDefect]) -> Option<String> {
    if defects.is_empty() {
        return None;
    }
    Some(
        defects
            .iter()
            .map(PathDefect::message)
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// A node is named by its displayed name; a blank name falls back to its id.
fn display_name(node: &crate::pipeline::NodeDef) -> String {
    if node.name.trim().is_empty() {
        node.id.clone()
    } else {
        node.name.clone()
    }
}

fn reachable<'a>(from: &'a str, adjacency: &HashMap<&'a str, Vec<&'a str>>) -> HashSet<&'a str> {
    let mut seen: HashSet<&str> = HashSet::from([from]);
    let mut queue = VecDeque::from([from]);
    while let Some(node) = queue.pop_front() {
        for next in adjacency.get(node).into_iter().flatten() {
            if seen.insert(next) {
                queue.push_back(next);
            }
        }
    }
    seen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::parse_pipeline;
    use pretty_assertions::assert_eq;
    use std::path::{Path, PathBuf};

    fn corpus_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/path-coherence")
    }

    #[derive(serde::Deserialize)]
    struct ExpectedDefect {
        kind: String,
        node: Option<String>,
        message: String,
    }

    fn kind_str(kind: PathDefectKind) -> &'static str {
        match kind {
            PathDefectKind::NoPath => "no_path",
            PathDefectKind::DeadEnd => "dead_end",
            PathDefectKind::Orphan => "orphan",
        }
    }

    /// The shared corpus (also read by the editor's TypeScript mirror): every
    /// case's defects — kind, culprit node, exact message, order — match.
    #[test]
    fn every_corpus_case_yields_its_expected_defects() {
        let manifest = std::fs::read_to_string(corpus_dir().join("expected.json")).unwrap();
        let cases: std::collections::BTreeMap<String, Vec<ExpectedDefect>> =
            serde_json::from_str(&manifest).unwrap();
        assert!(cases.len() >= 10, "the corpus lost cases: {}", cases.len());
        for (case, expected) in cases {
            let yaml = std::fs::read_to_string(corpus_dir().join(format!("{case}.yaml")))
                .unwrap_or_else(|e| panic!("{case}.yaml: {e}"));
            let parsed = parse_pipeline(&yaml).unwrap_or_else(|e| panic!("{case}: {e}"));
            let actual: Vec<(String, Option<String>, String)> = path_defects(&parsed.pipeline)
                .iter()
                .map(|d| (kind_str(d.kind).to_string(), d.node_id.clone(), d.message()))
                .collect();
            let wanted: Vec<(String, Option<String>, String)> = expected
                .into_iter()
                .map(|d| (d.kind, d.node, d.message))
                .collect();
            assert_eq!(actual, wanted, "corpus case {case}");
        }
    }

    /// Every corpus YAML is listed in the manifest — a fixture without an
    /// expectation would be silently untested.
    #[test]
    fn every_corpus_yaml_has_an_expectation() {
        let manifest = std::fs::read_to_string(corpus_dir().join("expected.json")).unwrap();
        let cases: serde_json::Map<String, serde_json::Value> =
            serde_json::from_str(&manifest).unwrap();
        for entry in std::fs::read_dir(corpus_dir()).unwrap().flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                let stem = path.file_stem().unwrap().to_str().unwrap();
                assert!(cases.contains_key(stem), "{stem}.yaml has no expectation");
            }
        }
    }

    /// Regression (ADR-0081 « Mesure »): every real pipeline the repository ships
    /// — its own `.pdo/pipelines/*` and the README media fixtures — stays launchable.
    #[test]
    fn every_real_pipeline_of_the_repo_stays_launchable() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dirs = [
            root.join(".pdo/pipelines"),
            root.join("scripts/readme-media/fixture/targets"),
        ];
        let mut checked = 0;
        for dir in dirs {
            for entry in std::fs::read_dir(&dir).unwrap().flatten() {
                let path = entry.path();
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                if !name.ends_with(".yaml") {
                    continue;
                }
                let yaml = std::fs::read_to_string(&path).unwrap();
                let parsed =
                    parse_pipeline(&yaml).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                let defects = path_defects(&parsed.pipeline);
                assert!(
                    defects.is_empty(),
                    "{} is not launchable: {:?}",
                    path.display(),
                    launch_refusal(&defects)
                );
                checked += 1;
            }
        }
        assert!(checked >= 6, "only {checked} real pipelines found");
    }

    #[test]
    fn refusal_lists_every_defect_in_order() {
        let yaml = std::fs::read_to_string(corpus_dir().join("start-not-wired.yaml")).unwrap();
        let parsed = parse_pipeline(&yaml).unwrap();
        assert_eq!(
            launch_refusal(&path_defects(&parsed.pipeline)).unwrap(),
            "Not launchable: no path from Start to End. \
             Not launchable: orphan node — node \"implementer\" is not reachable from Start."
        );
    }

    #[test]
    fn launchable_pipeline_has_no_refusal() {
        let yaml = std::fs::read_to_string(corpus_dir().join("linear-valid.yaml")).unwrap();
        let parsed = parse_pipeline(&yaml).unwrap();
        assert_eq!(launch_refusal(&path_defects(&parsed.pipeline)), None);
    }

    #[test]
    fn blank_node_name_falls_back_to_its_id() {
        let yaml = std::fs::read_to_string(corpus_dir().join("orphan-to-end.yaml"))
            .unwrap()
            .replace("name: stray", "name: \"\"");
        let parsed = parse_pipeline(&yaml).unwrap();
        assert_eq!(
            path_defects(&parsed.pipeline)[0].message(),
            "Not launchable: orphan node — node \"stray\" is not reachable from Start."
        );
    }

    #[test]
    fn an_edge_to_a_missing_node_is_left_to_the_dangling_check() {
        let yaml = std::fs::read_to_string(corpus_dir().join("linear-valid.yaml"))
            .unwrap()
            .replace(
                "edges:",
                "edges:\n  - source: { node: ghost, port: out }\n    target: { node: implementer, port: extra }",
            );
        let parsed = parse_pipeline(&yaml).unwrap();
        assert_eq!(path_defects(&parsed.pipeline), Vec::new());
    }
}
