//! The platform context's instance defaults, as `GET /settings` discloses them
//! (ADR-0015): may the daemon ask the release source for the latest published
//! version, and at which address do humans open PDO? The stored tier is handed
//! in by the settings context, which keeps it.

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

/// The address humans open PDO at, always set: `stored → env PDO_PUBLIC_URL →
/// default`, the default being the machine's address the daemon computed at
/// boot ([`pdo_kernel::public_url::default_public_url`]).
pub fn public_url(stored: Option<&str>, default: &str) -> Disclosed<String, String, String> {
    let env = pdo_kernel::public_url::env_public_url();
    Disclosed::new(
        pdo_kernel::public_url::resolve(stored, env.clone(), default),
        stored.map(str::to_string),
        env,
        default.to_string(),
    )
}
