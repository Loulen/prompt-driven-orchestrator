//! The environment tier of the telemetry setting, and the export period.

use std::collections::BTreeMap;
use std::time::Duration;

use pdo_run::application::stale::detector::parse_bool_setting;

use crate::domain::setting::{OtlpSetting, TelemetrySetting};

/// Middle tier of the switch's `stored → env → default(off)`.
pub const OTLP_ENV: &str = "PDO_TELEMETRY_OTLP";
/// Middle tier of the collector address's `stored → env → none`.
pub const OTLP_ENDPOINT_ENV: &str = "PDO_TELEMETRY_OTLP_ENDPOINT";
/// Middle tier of the `/metrics` switch's `stored → env → default(off)`.
pub const PROMETHEUS_ENV: &str = "PDO_TELEMETRY_PROMETHEUS";
/// Middle tier of « include the Actor »'s `stored → env → default(off)`.
pub const INCLUDE_ACTOR_ENV: &str = "PDO_TELEMETRY_INCLUDE_ACTOR";
/// Seconds between two metric exports; not a setting of the page.
pub const EXPORT_INTERVAL_ENV: &str = "PDO_TELEMETRY_EXPORT_INTERVAL_SECS";

pub const OTLP_DEFAULT: bool = false;
pub const PROMETHEUS_DEFAULT: bool = false;
pub const INCLUDE_ACTOR_DEFAULT: bool = false;
const EXPORT_INTERVAL_DEFAULT: Duration = Duration::from_secs(30);

fn env_flag(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .as_deref()
        .and_then(parse_bool_setting)
}

pub fn env_otlp() -> Option<bool> {
    env_flag(OTLP_ENV)
}

pub fn env_prometheus() -> Option<bool> {
    env_flag(PROMETHEUS_ENV)
}

pub fn env_include_actor() -> Option<bool> {
    env_flag(INCLUDE_ACTOR_ENV)
}

/// A switch, `stored → env → default`. A stored `0` is a decision that beats
/// the environment's `1`.
pub fn flag(stored: Option<i64>, env: Option<bool>, default: bool) -> bool {
    stored.map(|v| v != 0).or(env).unwrap_or(default)
}

pub fn env_otlp_endpoint() -> Option<String> {
    std::env::var(OTLP_ENDPOINT_ENV)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

/// The export period: the environment's when it is a positive number of
/// seconds, else 30 s.
pub fn export_interval() -> Duration {
    std::env::var(EXPORT_INTERVAL_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or(EXPORT_INTERVAL_DEFAULT)
}

/// The stored tier of the whole setting.
#[derive(Debug, Clone, Copy, Default)]
pub struct StoredTelemetry<'a> {
    pub otlp: Option<i64>,
    pub otlp_endpoint: Option<&'a str>,
    pub otlp_headers: Option<&'a BTreeMap<String, String>>,
    pub prometheus: Option<i64>,
    pub include_actor: Option<i64>,
}

/// The whole setting for the stored tier, the environment read now.
pub fn telemetry_setting(stored: StoredTelemetry<'_>) -> TelemetrySetting {
    TelemetrySetting {
        otlp: otlp_setting(
            stored.otlp,
            stored.otlp_endpoint,
            stored.otlp_headers.unwrap_or(&BTreeMap::new()),
        ),
        prometheus: flag(stored.prometheus, env_prometheus(), PROMETHEUS_DEFAULT),
        include_actor: flag(
            stored.include_actor,
            env_include_actor(),
            INCLUDE_ACTOR_DEFAULT,
        ),
    }
}

/// The OTLP setting for the stored tier, the environment read now.
pub fn otlp_setting(
    stored_enabled: Option<i64>,
    stored_endpoint: Option<&str>,
    headers: &BTreeMap<String, String>,
) -> OtlpSetting {
    OtlpSetting {
        enabled: flag(stored_enabled, env_otlp(), OTLP_DEFAULT),
        endpoint: stored_endpoint
            .map(str::to_string)
            .filter(|e| !e.trim().is_empty())
            .or_else(env_otlp_endpoint),
        headers: headers.clone(),
    }
}
