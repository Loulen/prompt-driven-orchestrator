//! The fires by Pipeline: an absorbed Pipeline's fires count under its
//! absorbent (#890), and an orphan fire (its Trigger was deleted, with no
//! cascade) is surfaced as `"(deleted trigger)"`, never dropped.

use std::collections::BTreeMap;

use super::PipelineFireCount;
use crate::domain::absorption::resolver::AbsorptionResolver;

/// The key of the fires whose Trigger no longer exists.
pub const DELETED_TRIGGER: &str = "(deleted trigger)";

/// Fold the fires per Pipeline key of the window (`rows`) under their
/// absorbents, most fired first.
pub fn fires_by_pipeline(
    rows: Vec<(String, i64)>,
    resolver: &AbsorptionResolver,
) -> Vec<PipelineFireCount> {
    let mut merged = BTreeMap::<String, i64>::new();
    for (pipeline_id, count) in rows {
        *merged
            .entry(resolver.pipeline(&pipeline_id).to_string())
            .or_default() += count;
    }
    let mut fires: Vec<PipelineFireCount> = merged
        .into_iter()
        .map(|(pipeline_id, count)| PipelineFireCount {
            name: resolver
                .stored_name(&pipeline_id)
                .unwrap_or(&pipeline_id)
                .to_string(),
            pipeline_id,
            count,
        })
        .collect();
    fires.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.pipeline_id.cmp(&b.pipeline_id))
    });
    fires
}
