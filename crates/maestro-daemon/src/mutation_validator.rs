use crate::{event_log, pipeline, scheduler};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MutationRejection {
    pub node_id: String,
    pub reason: String,
}

pub fn validate_run_mutation(
    old: &pipeline::PipelineDef,
    new: &pipeline::PipelineDef,
    run_state: &event_log::RunState,
) -> Vec<MutationRejection> {
    let mut rejections = Vec::new();

    let new_node_ids: std::collections::HashSet<&str> =
        new.nodes.iter().map(|n| n.id.as_str()).collect();

    for old_node in &old.nodes {
        if new_node_ids.contains(old_node.id.as_str()) {
            continue;
        }
        let status = run_state
            .nodes
            .get(&old_node.id)
            .map(|ns| &ns.status)
            .unwrap_or(&event_log::NodeStatus::Pending);

        if *status != event_log::NodeStatus::Pending {
            let status_str = match status {
                event_log::NodeStatus::Waiting => "waiting",
                event_log::NodeStatus::Running => "running",
                event_log::NodeStatus::Completed => "completed",
                event_log::NodeStatus::Failed => "failed",
                event_log::NodeStatus::AwaitingUser => "awaiting_user",
                event_log::NodeStatus::Stopped => "stopped",
                event_log::NodeStatus::Stale => "stale",
                event_log::NodeStatus::Pending => unreachable!(),
            };
            rejections.push(MutationRejection {
                node_id: old_node.id.clone(),
                reason: format!(
                    "cannot delete node '{}': status is '{}', must be 'pending'",
                    old_node.id, status_str
                ),
            });
        }
    }

    for new_node in &new.nodes {
        if new_node.node_type != pipeline::NodeType::Loop {
            continue;
        }
        let Some(loop_state) = run_state.loop_states.get(&new_node.id) else {
            continue;
        };

        let resolved_vars: std::collections::HashMap<String, serde_yaml::Value> =
            std::collections::HashMap::new();
        let new_max = scheduler::resolve_max_iter(new_node, &resolved_vars);

        if new_max < loop_state.current_iter {
            rejections.push(MutationRejection {
                node_id: new_node.id.clone(),
                reason: format!(
                    "cannot set max_iter={} on loop '{}': current iteration is {}",
                    new_max, new_node.id, loop_state.current_iter
                ),
            });
        }
    }

    // Live `max_iter` edit of a bounded loop region (ADR-0011 / #150, ADR-0007).
    // Editing a running region's bound IS allowed — it is the `extend_cycle` of
    // the Pipeline Manager. The only guard is the same consistency invariant the
    // legacy Loop node had: the new bound may not drop below the lap the region
    // is already on, which would strand the run mid-lap. The region runtime
    // counter is keyed by the region `id` in `loop_states`.
    let resolved_vars: std::collections::HashMap<String, serde_yaml::Value> =
        std::collections::HashMap::new();
    for region in &new.loops {
        if region.kind != pipeline::LoopKind::Bounded {
            continue;
        }
        let Some(loop_state) = run_state.loop_states.get(&region.id) else {
            continue;
        };
        let new_max = resolve_region_max_iter(region, &resolved_vars);
        if new_max < loop_state.current_iter {
            rejections.push(MutationRejection {
                node_id: region.id.clone(),
                reason: format!(
                    "cannot set max_iter={} on loop '{}': current iteration is {}",
                    new_max, region.id, loop_state.current_iter
                ),
            });
        }
    }

    rejections
}

/// Resolves a bounded region's `max_iter` to a concrete cap, mirroring the
/// node-based `scheduler::resolve_max_iter`: a number is taken as-is, a `$var`
/// string resolves against the run's variables, and an absent/invalid bound
/// falls back to the daemon default. Kept here (rather than on the region) so
/// the region and the legacy node agree on resolution during the live edit.
fn resolve_region_max_iter(
    region: &pipeline::LoopRegion,
    resolved_vars: &std::collections::HashMap<String, serde_yaml::Value>,
) -> i64 {
    match &region.max_iter {
        Some(serde_yaml::Value::Number(n)) => {
            n.as_i64().unwrap_or(crate::loop_region::DEFAULT_MAX_ITER)
        }
        Some(serde_yaml::Value::String(s)) => {
            if let Some(var_name) = s.strip_prefix('$') {
                resolved_vars
                    .get(var_name)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(crate::loop_region::DEFAULT_MAX_ITER)
            } else {
                s.parse::<i64>()
                    .unwrap_or(crate::loop_region::DEFAULT_MAX_ITER)
            }
        }
        _ => crate::loop_region::DEFAULT_MAX_ITER,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{event_log, pipeline};
    use std::collections::HashMap;

    fn simple_node(id: &str, node_type: pipeline::NodeType) -> pipeline::NodeDef {
        pipeline::NodeDef {
            id: id.to_string(),
            name: id.to_string(),
            node_type,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
        }
    }

    fn loop_node(id: &str, max_iter: i64) -> pipeline::NodeDef {
        pipeline::NodeDef {
            id: id.to_string(),
            name: id.to_string(),
            node_type: pipeline::NodeType::Loop,
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            view: None,
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                max_iter,
            ))),
            over: None,
        }
    }

    fn pipeline(nodes: Vec<pipeline::NodeDef>) -> pipeline::PipelineDef {
        pipeline::PipelineDef {
            name: "test".to_string(),
            version: None,
            variables: HashMap::new(),
            nodes,
            edges: vec![],
            loops: Vec::new(),
            prompt_required: true,
        }
    }

    fn run_state_with_nodes(nodes: Vec<(&str, event_log::NodeStatus)>) -> event_log::RunState {
        let mut state = event_log::RunState::new("run-1".into(), "test".into());
        for (id, status) in nodes {
            state.nodes.insert(
                id.to_string(),
                event_log::NodeState {
                    node_id: id.to_string(),
                    status,
                    iter: 1,
                    started_at: None,
                    completed_at: None,
                    failure_reason: None,
                    iterations: vec![],
                    frontmatter_retries: 0,
                    frontmatter_violations: vec![],
                },
            );
        }
        state
    }

    #[test]
    fn allows_deleting_pending_node() {
        let old = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
        ]);
        let new = pipeline(vec![simple_node("a", pipeline::NodeType::DocOnly)]);
        let rs = run_state_with_nodes(vec![
            ("a", event_log::NodeStatus::Running),
            ("b", event_log::NodeStatus::Pending),
        ]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(
            result.is_empty(),
            "deleting a pending node should be allowed"
        );
    }

    #[test]
    fn rejects_deleting_running_node() {
        let old = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
        ]);
        let new = pipeline(vec![simple_node("b", pipeline::NodeType::DocOnly)]);
        let rs = run_state_with_nodes(vec![
            ("a", event_log::NodeStatus::Running),
            ("b", event_log::NodeStatus::Pending),
        ]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_id, "a");
        assert!(result[0].reason.contains("running"));
    }

    #[test]
    fn rejects_deleting_completed_node() {
        let old = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
        ]);
        let new = pipeline(vec![simple_node("b", pipeline::NodeType::DocOnly)]);
        let rs = run_state_with_nodes(vec![
            ("a", event_log::NodeStatus::Completed),
            ("b", event_log::NodeStatus::Pending),
        ]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_id, "a");
        assert!(result[0].reason.contains("completed"));
    }

    #[test]
    fn rejects_deleting_failed_node() {
        let old = pipeline(vec![simple_node("a", pipeline::NodeType::DocOnly)]);
        let new = pipeline(vec![]);
        let rs = run_state_with_nodes(vec![("a", event_log::NodeStatus::Failed)]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_id, "a");
    }

    #[test]
    fn rejects_deleting_awaiting_user_node() {
        let old = pipeline(vec![simple_node("a", pipeline::NodeType::DocOnly)]);
        let new = pipeline(vec![]);
        let rs = run_state_with_nodes(vec![("a", event_log::NodeStatus::AwaitingUser)]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_id, "a");
    }

    #[test]
    fn allows_adding_new_nodes() {
        let old = pipeline(vec![simple_node("a", pipeline::NodeType::DocOnly)]);
        let new = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::CodeMutating),
        ]);
        let rs = run_state_with_nodes(vec![("a", event_log::NodeStatus::Running)]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(result.is_empty(), "adding new nodes should be allowed");
    }

    #[test]
    fn allows_adding_new_edges() {
        let old = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
        ]);
        let mut new = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
        ]);
        new.edges.push(pipeline::EdgeDef {
            source: pipeline::EdgeEndpoint {
                node: "a".into(),
                port: "out".into(),
            },
            target: pipeline::EdgeEndpoint {
                node: "b".into(),
                port: "in".into(),
            },
            reason: None,
            when: None,
            is_else: false,
            repeated: false,
            ..Default::default()
        });
        let rs = run_state_with_nodes(vec![("a", event_log::NodeStatus::Running)]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(result.is_empty(), "adding edges should be allowed");
    }

    #[test]
    fn allows_increasing_loop_max_iter() {
        let old = pipeline(vec![loop_node("review-loop", 5)]);
        let new = pipeline(vec![loop_node("review-loop", 10)]);
        let mut rs = run_state_with_nodes(vec![]);
        rs.loop_states.insert(
            "review-loop".to_string(),
            event_log::LoopState {
                loop_node_id: "review-loop".to_string(),
                current_iter: 3,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(
            result.is_empty(),
            "increasing max_iter above current_iter should be allowed"
        );
    }

    #[test]
    fn allows_max_iter_equal_to_current_iter() {
        let old = pipeline(vec![loop_node("review-loop", 5)]);
        let new = pipeline(vec![loop_node("review-loop", 3)]);
        let mut rs = run_state_with_nodes(vec![]);
        rs.loop_states.insert(
            "review-loop".to_string(),
            event_log::LoopState {
                loop_node_id: "review-loop".to_string(),
                current_iter: 3,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(
            result.is_empty(),
            "setting max_iter equal to current_iter should be allowed"
        );
    }

    #[test]
    fn rejects_max_iter_below_current_iter() {
        let old = pipeline(vec![loop_node("review-loop", 5)]);
        let new = pipeline(vec![loop_node("review-loop", 2)]);
        let mut rs = run_state_with_nodes(vec![]);
        rs.loop_states.insert(
            "review-loop".to_string(),
            event_log::LoopState {
                loop_node_id: "review-loop".to_string(),
                current_iter: 3,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_id, "review-loop");
        assert!(result[0].reason.contains("max_iter=2"));
        assert!(result[0].reason.contains("current iteration is 3"));
    }

    #[test]
    fn loop_without_active_state_allows_any_max_iter() {
        let old = pipeline(vec![loop_node("review-loop", 5)]);
        let new = pipeline(vec![loop_node("review-loop", 1)]);
        let rs = run_state_with_nodes(vec![]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(
            result.is_empty(),
            "loop without active state should accept any max_iter change"
        );
    }

    #[test]
    fn multiple_rejections_reported() {
        let old = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
            loop_node("loop-1", 5),
        ]);
        let new = pipeline(vec![loop_node("loop-1", 1)]);
        let mut rs = run_state_with_nodes(vec![
            ("a", event_log::NodeStatus::Running),
            ("b", event_log::NodeStatus::Completed),
        ]);
        rs.loop_states.insert(
            "loop-1".to_string(),
            event_log::LoopState {
                loop_node_id: "loop-1".to_string(),
                current_iter: 3,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 3, "should report all three rejections");
    }

    fn region(id: &str, max_iter: i64) -> pipeline::LoopRegion {
        pipeline::LoopRegion {
            id: id.to_string(),
            kind: pipeline::LoopKind::Bounded,
            members: vec!["impl".to_string(), "rev".to_string()],
            max_iter: Some(serde_yaml::Value::Number(serde_yaml::Number::from(
                max_iter,
            ))),
            over: None,
        }
    }

    fn pipeline_with_region(region: pipeline::LoopRegion) -> pipeline::PipelineDef {
        let mut p = pipeline(vec![
            simple_node("impl", pipeline::NodeType::CodeMutating),
            simple_node("rev", pipeline::NodeType::DocOnly),
        ]);
        p.loops = vec![region];
        p
    }

    #[test]
    fn allows_increasing_region_max_iter_live() {
        // ADR-0007 (b): editing a live bounded region's max_iter is allowed — it
        // is the `extend_cycle` of the Pipeline Manager. The region runtime
        // counter is keyed by the region id (loop_states), at iter 2.
        let old = pipeline_with_region(region("review_loop", 3));
        let new = pipeline_with_region(region("review_loop", 6));
        let mut rs = run_state_with_nodes(vec![]);
        rs.loop_states.insert(
            "review_loop".to_string(),
            event_log::LoopState {
                loop_node_id: "review_loop".to_string(),
                current_iter: 2,
                max_iter: 3,
                break_received: false,
                done: false,
            },
        );

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(
            result.is_empty(),
            "extending a live region's max_iter must be allowed"
        );
    }

    #[test]
    fn rejects_region_max_iter_below_current_iter() {
        // Lowering a live region's max_iter below the lap it is already on would
        // strand the run mid-lap; reject it (ADR-0007 consistency invariant).
        let old = pipeline_with_region(region("review_loop", 5));
        let new = pipeline_with_region(region("review_loop", 2));
        let mut rs = run_state_with_nodes(vec![]);
        rs.loop_states.insert(
            "review_loop".to_string(),
            event_log::LoopState {
                loop_node_id: "review_loop".to_string(),
                current_iter: 3,
                max_iter: 5,
                break_received: false,
                done: false,
            },
        );

        let result = validate_run_mutation(&old, &new, &rs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].node_id, "review_loop");
        assert!(result[0].reason.contains("max_iter=2"));
        assert!(result[0].reason.contains("current iteration is 3"));
    }

    #[test]
    fn region_without_active_state_allows_any_max_iter() {
        // No live counter for the region (not yet run / not iterating) ⇒ any
        // max_iter is accepted; the guard only bites a live region.
        let old = pipeline_with_region(region("review_loop", 5));
        let new = pipeline_with_region(region("review_loop", 1));
        let rs = run_state_with_nodes(vec![]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(result.is_empty());
    }

    #[test]
    fn node_not_in_run_state_treated_as_pending() {
        let old = pipeline(vec![
            simple_node("a", pipeline::NodeType::DocOnly),
            simple_node("b", pipeline::NodeType::DocOnly),
        ]);
        let new = pipeline(vec![simple_node("a", pipeline::NodeType::DocOnly)]);
        let rs = run_state_with_nodes(vec![("a", event_log::NodeStatus::Running)]);

        let result = validate_run_mutation(&old, &new, &rs);
        assert!(
            result.is_empty(),
            "node not present in run_state should be treated as pending (deletable)"
        );
    }
}
