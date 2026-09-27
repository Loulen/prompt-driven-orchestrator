//! Reading the registry: the list, one pipeline.

use pdo_pipeline::application::PipelineError;
use pdo_pipeline::domain::pipeline::VariableType;

use crate::fakes::*;

#[test]
fn the_list_is_sorted_by_name_and_carries_each_pipelines_metadata() {
    let world = World::new();
    world.registry.put("zeta", &pipeline_yaml("zeta"));
    world.registry.put(
        "alpha",
        &pipeline_yaml("Alpha").replace(
            "version: \"1.0\"\n",
            "version: \"1.0\"\nprompt_required: false\nvariables:\n  retries: 3\n",
        ),
    );
    world.registry.put("broken", "name: [unclosed");

    let list = world.pipelines.list().unwrap();
    let ids: Vec<&str> = list.iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids, ["alpha", "broken", "zeta"]);
    let alpha = &list[0];
    assert_eq!(alpha.name, "Alpha");
    assert_eq!(alpha.node_count, 2);
    assert!(!alpha.prompt_required);
    assert_eq!(alpha.variables["retries"].var_type, VariableType::Int);
    assert_eq!(alpha.modified.as_deref(), Some("2026-09-27T00:00:00Z"));
    // A file that does not parse is still listed, under its stem.
    let broken = &list[1];
    assert_eq!(broken.name, "broken");
    assert_eq!(broken.node_count, 0);
    assert!(broken.prompt_required);
    assert!(list[2].prompt_required);
}

#[test]
fn listing_unifies_the_retired_registries_first() {
    let world = World::new();
    world.pipelines.list().unwrap();
    assert_eq!(*world.registry.unified.lock().unwrap(), 1);
}

#[test]
fn a_registry_with_no_directory_lists_nothing_but_says_so() {
    let world = World::with(
        Host::default(),
        Registry {
            dir: None,
            ..Registry::default()
        },
        Bank::default(),
    );
    assert!(matches!(
        world.pipelines.list(),
        Err(PipelineError::NoRegistry)
    ));
}

#[test]
fn the_detail_parses_the_pipeline_and_reads_the_prompts_of_its_nodes() {
    let world = World::new();
    world
        .registry
        .put("my-pipe", &pipeline_with_worker("my-pipe"));
    world.registry.put_prompt("my-pipe", "worker", "Do it.");
    world.registry.put_prompt("my-pipe", "gone", "An orphan.");

    let detail = world.pipelines.detail("my-pipe").unwrap();
    assert_eq!(detail.id, "my-pipe");
    assert_eq!(detail.pipeline.name, "my-pipe");
    assert_eq!(detail.pipeline.nodes.len(), 3);
    assert_eq!(detail.prompts.len(), 1);
    assert_eq!(detail.prompts["worker"], "Do it.");
    assert_eq!(detail.path.to_string_lossy(), "/registry/my-pipe.yaml");
}

#[test]
fn the_detail_materializes_the_region_of_an_undeclared_cycle() {
    let world = World::new();
    let yaml = pipeline_with_worker("cyclic").replace(
        "edges:\n",
        "edges:\n  - source: { node: worker, port: out }\n    target: { node: worker, port: again }\n",
    );
    world.registry.put("cyclic", &yaml);
    let detail = world.pipelines.detail("cyclic").unwrap();
    assert_eq!(detail.pipeline.loops.len(), 1);
    assert_eq!(detail.pipeline.loops[0].members, ["worker"]);
}

#[test]
fn an_unknown_or_unparsable_pipeline_is_refused() {
    let world = World::new();
    assert!(matches!(
        world.pipelines.detail("nonexistent"),
        Err(PipelineError::NotFound)
    ));
    world.registry.put("bad", "nodes: [");
    assert!(matches!(
        world.pipelines.detail("bad"),
        Err(PipelineError::Invalid(_))
    ));
}
