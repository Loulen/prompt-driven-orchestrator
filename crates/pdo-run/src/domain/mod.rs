//! The pure rules of a Run. Nothing here does I/O.
//!
//! - [`event`]: what the event log records.
//! - [`state`] and [`projection`]: the Run as the fold of its events.
//! - The graph a Run walks: [`graph_resolver`], [`loop_region`],
//!   [`edge_router`], [`switch_router`], [`condition`], [`variable_resolver`],
//!   [`input_resolution`], [`blackboard`], [`on_demand`] (ADR-0086).
//! - What a Run allows: [`transition_guard`], [`admission`] (the session cap),
//!   [`completion_refusal`], [`retry_verdict`], [`restart_verdict`],
//!   [`reap_policy`], [`auto_fail`], [`child_awaiting`].
//! - [`review_comment`]: the review comments a Run folds.
//! - [`run_summary`]: the Résumé de Run the lists read (ADR-0084), and
//!   [`run_root`]: the Run at the top of a chain of parents.
//! - [`live_session`]: a node's facts for the gestures that speak to its agent.
//! - [`presence`]: who has a Run open in a browser (API 0.12).
//! - What a Run costs: [`prices`] (the price table's tiers) and [`run_cost`]
//!   (the contributions and a transcript's usage).

pub mod admission;
pub mod auto_fail;
pub mod blackboard;
pub mod child_awaiting;
pub mod completion_refusal;
pub mod condition;
pub mod declared_wait;
pub mod edge_router;
pub mod event;
pub mod graph_resolver;
pub mod input_resolution;
pub mod live_session;
pub mod loop_region;
pub mod merge_action;
pub mod on_demand;
pub mod presence;
pub mod prices;
pub mod projection;
pub mod reap_policy;
pub mod recovery;
pub mod restart_verdict;
pub mod retry_verdict;
pub mod review_comment;
pub mod run_activity;
pub mod run_cost;
pub mod run_refs;
pub mod run_root;
pub mod run_stall;
pub mod run_summary;
pub mod scheduler;
pub mod state;
pub mod stream_liveness;
pub mod switch_router;
pub mod transition_guard;
pub mod variable_resolver;
