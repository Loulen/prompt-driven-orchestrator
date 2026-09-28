//! The harnesses: the settings page's descriptors block (each harness's offer,
//! the refused descriptors, the file's path — the settings surface is where a
//! broken descriptor is ALWAYS said), the check a default harness must pass,
//! and the catalogues' warm-up.

use pdo_harness::adapters::descriptor_file::{load, load_or_builtin};
use pdo_harness::adapters::http::descriptors_view::descriptors_view;
use pdo_harness::application::harness_offers::{list_offers, warm_catalogues};
use pdo_harness::application::read_catalogue::CatalogueReader;
use serde_json::{Map, Value};

use super::{section, HostHome};
use crate::application::ports::{BoxFuture, Harnesses, SettingsSection};
use crate::domain::instance_config::InstanceConfig;

pub struct HarnessCatalogue {
    pub home: HostHome,
    /// Per-harness offered-catalogue cache (ADR-0053 §3): at most one
    /// `--version` re-check per harness per window, a full `--help` re-probe
    /// only when the version changed. Never persisted.
    pub reader: CatalogueReader,
}

impl SettingsSection for HarnessCatalogue {
    fn disclose<'a>(
        &'a self,
        _stored: &'a InstanceConfig,
    ) -> BoxFuture<'a, Result<Map<String, Value>, String>> {
        Box::pin(async move {
            let home = self.home.path();
            let registry = load_or_builtin(home.as_deref());
            let offers = list_offers(&self.reader, &registry).await;
            Ok(section([(
                "harness_descriptors",
                descriptors_view(home.as_deref(), &registry, &offers),
            )]))
        })
    }
}

impl Harnesses for HarnessCatalogue {
    fn check_default(&self, name: &str) -> Result<(), String> {
        let registry = load(&self.home.path().unwrap_or_default());
        if registry.resolve(name).is_some() {
            return Ok(());
        }
        Err(format!(
            "unknown default harness `{name}`: it resolves to no embedded or declared \
             harness — a default that does not resolve would break every spawn that \
             falls through to it. Known harnesses: {}",
            registry.names().join(", ")
        ))
    }

    fn warm(&self) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let home = self.home.path();
            warm_catalogues(&self.reader, &load_or_builtin(home.as_deref())).await;
        })
    }
}
