use std::collections::BTreeMap;

use super::LABEL;

/// A pure transition: no I/O, no framework.
pub fn next_status(current: &str) -> &'static str {
    let _ = (LABEL, BTreeMap::<u8, u8>::new());
    match current {
        "pending" => "running",
        _ => "completed",
    }
}
