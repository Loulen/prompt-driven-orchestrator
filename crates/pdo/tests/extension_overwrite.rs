//! ADR-0085 — « Overwrite default pipeline » asks both Operations: `run` ×
//! `edit_pipeline` (the Run's edit) and `pipeline` × `update` (the shared
//! pipeline's save). A test extension refusing either one refuses the
//! overwrite, and the shared pipeline stays as it was.

use pdo::extension_api::{ResourceKind, Verb};

use crate::common::flow::{create_run, seed_solo};
use crate::common::TestDaemon;
use crate::test_extension::{Refusal, TestExtension};

const PIPELINE: &str = "shared";

async fn overwrite_with(refusal: Refusal) -> (u16, String, String) {
    let (ext, _control) = TestExtension::new(refusal);
    let daemon = TestDaemon::spawn_with_extensions(
        |repo| {
            crate::common::flow::git_init(repo)?;
            seed_solo(repo, &[(PIPELINE, false)])
        },
        ext.boxed(),
    )
    .await
    .unwrap();
    let shared = daemon
        .repo_root()
        .join(".pdo/pipelines")
        .join(format!("{PIPELINE}.yaml"));
    let before = std::fs::read_to_string(&shared).unwrap();
    let run_id = create_run(&daemon, PIPELINE).await;
    // The Run's snapshot differs from the shared pipeline.
    let snapshot = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(&run_id)
        .join("pipeline.yaml");
    std::fs::write(
        &snapshot,
        before.replace("version: \"1.0\"", "version: \"9.0\""),
    )
    .unwrap();

    let resp = reqwest::Client::new()
        .post(format!(
            "{}/runs/{}/pipeline/overwrite-default",
            daemon.url(),
            run_id
        ))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    (status, before, std::fs::read_to_string(&shared).unwrap())
}

#[tokio::test]
async fn refusing_the_runs_edit_refuses_the_overwrite() {
    let (status, before, after) = overwrite_with(Refusal::Only(
        ResourceKind::Run,
        Verb::Action("edit_pipeline".into()),
    ))
    .await;
    assert_eq!(status, 403);
    assert_eq!(after, before, "the shared pipeline is untouched");
}

#[tokio::test]
async fn refusing_the_pipelines_update_refuses_the_overwrite() {
    let (status, before, after) =
        overwrite_with(Refusal::Only(ResourceKind::Pipeline, Verb::Update)).await;
    assert_eq!(status, 403);
    assert_eq!(after, before, "the shared pipeline is untouched");
}

#[tokio::test]
async fn allowing_both_overwrites_the_shared_pipeline() {
    let (status, before, after) = overwrite_with(Refusal::Nothing).await;
    assert_eq!(status, 200);
    assert_ne!(after, before);
    assert!(after.contains("version: \"9.0\""));
}
