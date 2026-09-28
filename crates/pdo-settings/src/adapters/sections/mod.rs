//! The sections of the settings page, one per context that discloses
//! something on it. Each one hands the stored tier to the owning context's own
//! disclosure (`<context>::adapters::instance_defaults`) and names the keys; no
//! knob is resolved here.

mod defaults;
mod harness;
mod prices;
mod sandbox;

use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Map, Value};

pub use defaults::{
    PlatformDefaults, ReviewDefaults, RunDefaults, SessionDefaults, TriggerDefaults,
};
pub use harness::HarnessCatalogue;
pub use prices::PriceTableSection;
pub use sandbox::SandboxSection;

/// The home the page reads under: the per-daemon override (a test's temporary
/// directory), else `$HOME`; `None` when neither is set.
#[derive(Debug, Clone, Default)]
pub struct HostHome {
    pub override_home: Option<PathBuf>,
}

impl HostHome {
    pub fn path(&self) -> Option<PathBuf> {
        self.override_home
            .clone()
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    }
}

/// A section's keys, each serialized as the page sends it.
fn section<const N: usize>(entries: [(&str, Value); N]) -> Map<String, Value> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn json<T: Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}
