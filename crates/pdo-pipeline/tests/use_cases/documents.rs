//! The portable document (ADR-0059) and its skills sidecar (#673).

use std::collections::BTreeMap;

use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::pipeline::parse_pipeline;
use pdo_skill::domain::selection::SkillRef;
use pdo_skill::domain::sidecar::{SidecarImportReport, SidecarSkill};

use crate::fakes::*;

fn with_skill(name: &str, skill_id: &str) -> String {
    pipeline_with_worker(name).replace(
        "    type: agent\n",
        &format!("    type: agent\n    skills:\n      - {{ id: {skill_id}, name: Old label }}\n"),
    )
}

#[test]
fn an_exported_document_imports_back_to_the_same_pipeline() {
    let world = World::new();
    world.registry.put("round", &pipeline_with_worker("round"));
    world.registry.put_prompt("round", "worker", "Work hard.");
    let document = world.pipelines.export_document("round").unwrap();

    let other = World::new();
    let imported = tokio_test_block(other.pipelines.import_document(&operator(), &document, &[]));
    assert_eq!(imported.id, "round");
    let entry = other.registry.get("round").unwrap();
    let original = parse_pipeline(&pipeline_with_worker("round"))
        .unwrap()
        .pipeline;
    let back = parse_pipeline(&entry.yaml).unwrap().pipeline;
    assert_eq!(
        serde_json::to_value(&back).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(entry.prompts["worker"], "Work hard.");
}

fn tokio_test_block<F: std::future::Future<Output = Result<T, PipelineError>>, T>(f: F) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
        .unwrap()
}

#[tokio::test]
async fn an_import_takes_the_first_free_identity_and_relabels_the_skills_the_bank_kept() {
    let bank = Bank {
        report: SidecarImportReport {
            kept: vec![SkillRef {
                id: "s1".into(),
                name: "Bank label".into(),
            }],
            warnings: vec!["skills.s1: kept".into()],
            ..SidecarImportReport::default()
        },
        ..Bank::default()
    };
    let source = World::new();
    source.registry.put("shared", &with_skill("shared", "s1"));
    let document = source.pipelines.export_document("shared").unwrap();

    let world = World::with(Host::default(), Registry::default(), bank);
    world.registry.put("shared", &pipeline_yaml("shared"));
    let sidecar = [SidecarSkill {
        id: "s1".into(),
        files: BTreeMap::new(),
    }];
    let imported = world
        .pipelines
        .import_document(&operator(), &document, &sidecar)
        .await
        .unwrap();
    assert_eq!(imported.id, "shared-2");
    assert_eq!(imported.warnings, ["skills.s1: kept"]);
    let yaml = world.registry.get("shared-2").unwrap().yaml;
    let parsed = parse_pipeline(&yaml).unwrap().pipeline;
    assert_eq!(parsed.name, "shared (2)");
    let worker = parsed.nodes.iter().find(|n| n.id == "worker").unwrap();
    assert_eq!(worker.skills[0].name, "Bank label");
    // The bank's import folder is named after the document's own name.
    let imports = world.bank.imported.lock().unwrap();
    assert_eq!(imports[0].0, "shared");
    assert_eq!(imports[0].2, 1);
}

#[tokio::test]
async fn a_document_that_does_not_read_is_a_bad_request_and_writes_nothing() {
    let world = World::new();
    assert!(matches!(
        world
            .pipelines
            .import_document(&operator(), "not: a document", &[])
            .await,
        Err(PipelineError::BadRequest(_))
    ));
    assert!(world.registry.ids().is_empty());
}

#[test]
fn the_sidecar_carries_the_referenced_skills_the_bank_holds() {
    let bank = Bank {
        held: vec![SidecarSkill {
            id: "s1".into(),
            files: BTreeMap::from([("SKILL.md".to_string(), b"---\n".to_vec())]),
        }],
        ..Bank::default()
    };
    let world = World::with(Host::default(), Registry::default(), bank);
    world.registry.put("with", &with_skill("with", "s1"));
    world
        .registry
        .put("absent", &with_skill("absent", "nowhere"));
    world
        .registry
        .put("without", &pipeline_with_worker("without"));

    let sidecar = world.pipelines.document_skills("with").unwrap().unwrap();
    assert_eq!(sidecar.pipeline_name, "with");
    assert_eq!(sidecar.skills.len(), 1);
    let sidecar = world.pipelines.document_skills("absent").unwrap().unwrap();
    assert!(sidecar.skills.is_empty());
    assert!(world
        .pipelines
        .document_skills("without")
        .unwrap()
        .is_none());
}
