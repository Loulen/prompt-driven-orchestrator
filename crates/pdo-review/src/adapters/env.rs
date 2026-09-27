//! The environment tier of the review's instance setting.

use pdo_run::application::stale::detector::parse_bool_setting;

use crate::domain::comments::decision::agent_can_resolve;

/// Middle tier of `review_agent_can_resolve`'s `stored → env → default(false)`.
pub const AGENT_CAN_RESOLVE_ENV: &str = "PDO_REVIEW_AGENT_CAN_RESOLVE";

/// The env tier, through the shared boolean parser so a typo falls through to
/// the default rather than silently meaning `false`.
pub fn env_agent_can_resolve() -> Option<bool> {
    std::env::var(AGENT_CAN_RESOLVE_ENV)
        .ok()
        .as_deref()
        .and_then(parse_bool_setting)
}

/// `review_agent_can_resolve` for the stored column `stored`, the
/// environment read now.
pub fn agent_can_resolve_with(stored: Option<i64>) -> bool {
    agent_can_resolve(stored, env_agent_can_resolve())
}
