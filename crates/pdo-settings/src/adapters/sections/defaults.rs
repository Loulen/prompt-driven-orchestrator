//! The sections that are only instance defaults: the run, session, trigger,
//! review and platform contexts' knobs over the stored tier.

use pdo_platform::adapters::instance_defaults as platform;
use pdo_review::adapters::instance_defaults as review;
use pdo_run::adapters::instance_defaults as run;
use pdo_session::adapters::instance_defaults as session;
use pdo_telemetry::adapters::instance_defaults as telemetry;
use pdo_trigger::adapters::instance_defaults as trigger;
use serde_json::{Map, Value};

use super::{json, section};
use crate::application::ports::{BoxFuture, SettingsSection};
use crate::domain::instance_config::InstanceConfig;

type Keys<'a> = BoxFuture<'a, Result<Map<String, Value>, String>>;

/// The session cap, the turn-end auto-completion, the auto-naming, the
/// instance tier of `auto_fail`, the manager's auto-start and pin, the
/// attachment budget.
pub struct RunDefaults;

impl SettingsSection for RunDefaults {
    fn disclose<'a>(&'a self, c: &'a InstanceConfig) -> Keys<'a> {
        Box::pin(async move {
            Ok(section([
                ("session_cap", json(run::session_cap(c.session_cap))),
                (
                    "autocomplete_turn_end",
                    json(run::autocomplete_turn_end(c.autocomplete_turn_end)),
                ),
                (
                    "default_auto_name",
                    json(run::default_auto_name(c.default_auto_name)),
                ),
                ("auto_fail", json(run::auto_fail(c.auto_fail))),
                (
                    "manager_enabled",
                    json(run::manager_enabled(c.manager_enabled)),
                ),
                (
                    "manager_profile",
                    json(run::manager_profile(c.manager_profile.as_deref())),
                ),
                (
                    "max_attachments_mb",
                    json(run::max_attachments_mb(c.max_attachments_mb)),
                ),
            ]))
        })
    }
}

/// The reaper's TTLs, the default model and harness, the per-harness models.
pub struct SessionDefaults;

impl SettingsSection for SessionDefaults {
    fn disclose<'a>(&'a self, c: &'a InstanceConfig) -> Keys<'a> {
        Box::pin(async move {
            Ok(section([
                (
                    "reaper_ttl_secs",
                    json(session::reaper_ttl_secs(c.reaper_ttl_secs)),
                ),
                (
                    "libassist_idle_ttl_secs",
                    json(session::libassist_idle_ttl_secs(c.libassist_idle_ttl_secs)),
                ),
                (
                    "default_model",
                    json(session::default_model(c.default_model.as_deref())),
                ),
                (
                    "default_harness",
                    json(session::default_harness(c.default_harness.as_deref())),
                ),
                (
                    "default_harness_model",
                    json(session::default_harness_model(
                        &c.default_harness_model,
                        c.default_model.as_deref(),
                    )),
                ),
            ]))
        })
    }
}

/// The guard timeout.
pub struct TriggerDefaults;

impl SettingsSection for TriggerDefaults {
    fn disclose<'a>(&'a self, c: &'a InstanceConfig) -> Keys<'a> {
        Box::pin(async move {
            Ok(section([(
                "guard_timeout_secs",
                json(trigger::guard_timeout_secs(c.guard_timeout_secs)),
            )]))
        })
    }
}

/// Whether an agent may resolve a review comment itself.
pub struct ReviewDefaults;

impl SettingsSection for ReviewDefaults {
    fn disclose<'a>(&'a self, c: &'a InstanceConfig) -> Keys<'a> {
        Box::pin(async move {
            Ok(section([(
                "review_agent_can_resolve",
                json(review::review_agent_can_resolve(c.review_agent_can_resolve)),
            )]))
        })
    }
}

/// Whether the daemon may ask the release source for a newer version.
pub struct PlatformDefaults;

impl SettingsSection for PlatformDefaults {
    fn disclose<'a>(&'a self, c: &'a InstanceConfig) -> Keys<'a> {
        Box::pin(async move {
            Ok(section([(
                "update_check",
                json(platform::update_check(c.update_check)),
            )]))
        })
    }
}

/// The OTLP export: its switch, its collector, its headers by name.
pub struct TelemetryDefaults;

impl SettingsSection for TelemetryDefaults {
    fn disclose<'a>(&'a self, c: &'a InstanceConfig) -> Keys<'a> {
        Box::pin(async move {
            Ok(section([
                (
                    "telemetry_otlp",
                    json(telemetry::telemetry_otlp(c.telemetry_otlp)),
                ),
                (
                    "telemetry_otlp_endpoint",
                    json(telemetry::telemetry_otlp_endpoint(
                        c.telemetry_otlp_endpoint.as_deref(),
                    )),
                ),
                (
                    "telemetry_otlp_headers",
                    json(telemetry::telemetry_otlp_headers(&c.telemetry_otlp_headers)),
                ),
            ]))
        })
    }
}
