//! The telemetry's instance defaults, as `GET /settings` discloses them
//! (ADR-0015): the OTLP switch and the collector's address with their tiers,
//! the headers by name only. The stored tier is handed in by the settings
//! context, which keeps it.

use std::collections::BTreeMap;

use pdo_kernel::instance_default::{Disclosed, Flag, Text};

use super::env::{env_otlp, env_otlp_endpoint, otlp_setting, OTLP_DEFAULT};
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
