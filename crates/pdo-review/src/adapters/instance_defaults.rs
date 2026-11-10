//! The review context's instance default, as `GET /settings` discloses it
//! (ADR-0015): may an agent's `pdo review reply --resolved` resolve the comment
//! directly? The stored tier is handed in by the settings context, which keeps
//! it.

use pdo_kernel::instance_default::{Disclosed, Flag};

use super::env::{agent_can_resolve_with, env_agent_can_resolve};
use crate::domain::comments::decision::AGENT_CAN_RESOLVE_DEFAULT;

/// Default OFF: the human resolves, the agent proposes (ADR-0067 §4).
pub fn review_agent_can_resolve(stored: Option<i64>) -> Flag {
    Disclosed::new(
        agent_can_resolve_with(stored),
        stored.map(|v| v != 0),
        env_agent_can_resolve(),
        AGENT_CAN_RESOLVE_DEFAULT,
    )
}
