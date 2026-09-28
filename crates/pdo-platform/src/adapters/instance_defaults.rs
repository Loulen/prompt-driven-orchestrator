//! The platform context's instance default, as `GET /settings` discloses it
//! (ADR-0015): may the daemon ask the release source for the latest published
//! version? The stored tier is handed in by the settings context, which keeps
//! it.

use pdo_kernel::instance_default::{Disclosed, Flag};

use super::env::env_update_check;
use crate::domain::update_check::{resolve_update_check, UPDATE_CHECK_DEFAULT};

/// Default ON.
pub fn update_check(stored: Option<i64>) -> Flag {
    let env = env_update_check();
    Disclosed::new(
        resolve_update_check(stored, env),
        stored.map(|v| v != 0),
        env,
        UPDATE_CHECK_DEFAULT,
    )
}
