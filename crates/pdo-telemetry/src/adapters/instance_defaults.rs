//! The telemetry's instance defaults, as `GET /settings` discloses them
//! (ADR-0015): the OTLP switch and the collector's address with their tiers,
//! the headers by name only, the `/metrics` switch and « include the Actor ». The stored tier is handed in by the settings
//! context, which keeps it.

use std::collections::BTreeMap;

use pdo_kernel::instance_default::{Disclosed, Flag, Text};

use super::env::{
    env_include_actor, env_otlp, env_otlp_endpoint, env_prometheus, flag, otlp_setting,
    INCLUDE_ACTOR_DEFAULT, OTLP_DEFAULT, PROMETHEUS_DEFAULT,
};
use crate::domain::setting::{header_views, OtlpHeaderView};

/// Off by default.
pub fn telemetry_otlp(stored: Option<i64>) -> Flag {
    Disclosed::new(
        otlp_setting(stored, None, &BTreeMap::new()).enabled,
        stored.map(|v| v != 0),
        env_otlp(),
        OTLP_DEFAULT,
    )
}

/// No collector by default.
pub fn telemetry_otlp_endpoint(stored: Option<&str>) -> Text {
    let stored = stored.map(str::to_string).filter(|e| !e.trim().is_empty());
    Disclosed::new(
        otlp_setting(None, stored.as_deref(), &BTreeMap::new()).endpoint,
        stored,
        env_otlp_endpoint(),
        None,
    )
}

/// The headers, named, never valued.
pub fn telemetry_otlp_headers(stored: &BTreeMap<String, String>) -> Vec<OtlpHeaderView> {
    header_views(stored)
}

/// Off by default.
pub fn telemetry_prometheus(stored: Option<i64>) -> Flag {
    let env = env_prometheus();
    Disclosed::new(
        flag(stored, env, PROMETHEUS_DEFAULT),
        stored.map(|v| v != 0),
        env,
        PROMETHEUS_DEFAULT,
    )
}

/// Off by default.
pub fn telemetry_include_actor(stored: Option<i64>) -> Flag {
    let env = env_include_actor();
    Disclosed::new(
        flag(stored, env, INCLUDE_ACTOR_DEFAULT),
        stored.map(|v| v != 0),
        env,
        INCLUDE_ACTOR_DEFAULT,
    )
}
