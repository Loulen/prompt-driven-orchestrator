//! The environment tier of the OTLP export setting, and the export period.

use std::collections::BTreeMap;
use std::time::Duration;

use pdo_run::application::stale::detector::parse_bool_setting;

use crate::domain::setting::OtlpSetting;

/// Middle tier of the switch's `stored → env → default(off)`.
pub const OTLP_ENV: &str = "PDO_TELEMETRY_OTLP";
/// Middle tier of the collector address's `stored → env → none`.
pub const OTLP_ENDPOINT_ENV: &str = "PDO_TELEMETRY_OTLP_ENDPOINT";
/// Seconds between two metric exports; not a setting of the page.
pub const EXPORT_INTERVAL_ENV: &str = "PDO_TELEMETRY_EXPORT_INTERVAL_SECS";

pub const OTLP_DEFAULT: bool = false;
const EXPORT_INTERVAL_DEFAULT: Duration = Duration::from_secs(30);

pub fn env_otlp() -> Option<bool> {
    std::env::var(OTLP_ENV)
        .ok()
        .as_deref()
        .and_then(parse_bool_setting)
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

/// The setting for the stored tier, the environment read now. A stored `0`
/// is a decision that beats the environment's `1`.
pub fn otlp_setting(
    stored_enabled: Option<i64>,
    stored_endpoint: Option<&str>,
    headers: &BTreeMap<String, String>,
) -> OtlpSetting {
    OtlpSetting {
        enabled: stored_enabled
            .map(|v| v != 0)
            .or_else(env_otlp)
            .unwrap_or(OTLP_DEFAULT),
        endpoint: stored_endpoint
            .map(str::to_string)
            .filter(|e| !e.trim().is_empty())
            .or_else(env_otlp_endpoint),
        headers: headers.clone(),
    }
}
