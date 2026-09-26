//! What each resolved harness offers the pickers (#586, #616, #705, #798): its
//! provenance, whether its binary is installed, and its catalogue.

use crate::application::read_catalogue::CatalogueReader;
use crate::domain::catalogue::CachedCatalogue;
use crate::domain::registry::{HarnessRegistry, HarnessSource};

/// One harness's offer, in registry order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessOffer {
    pub name: String,
    pub source: HarnessSource,
    /// Whether its binary resolves on the harness probe `PATH`. An uninstalled
    /// harness carries an empty offer: the free-text fallback, a declared absence.
    pub installed: bool,
    pub cached: CachedCatalogue,
    /// The served effort-axis fact: the binary enumerates effort stops, OR the
    /// launch template carries an `{effort}` hole — a harness that takes effort
    /// but prints no list still offers the axis.
    pub has_effort: bool,
}

/// The offer of every harness `registry` resolves. The binary probe — the same
/// the spawn seam runs — decides `installed`, so the picker greys a harness that
/// would only fail-fast at launch.
pub async fn list_offers(
    reader: &CatalogueReader,
    registry: &HarnessRegistry,
) -> Vec<HarnessOffer> {
    let mut offers = Vec::new();
    for h in registry.listing() {
        let installed = reader.installed(&h.binary);
        let cached = if installed {
            reader.read(&h.name, &h.binary).await
        } else {
            CachedCatalogue::default()
        };
        let has_effort_hole = registry
            .resolve(&h.name)
            .map(|d| d.has_effort_hole())
            .unwrap_or(false);
        let has_effort = cached.catalogue.has_effort_axis() || has_effort_hole;
        offers.push(HarnessOffer {
            name: h.name,
            source: h.source,
            installed,
            cached,
            has_effort,
        });
    }
    offers
}

/// Probe every installed harness's catalogue once, populating the cache, so the
/// first `/settings` fetch answers warm. A harness whose binary is absent is
/// skipped — no probe, an empty offer, the free-text field.
pub async fn warm_catalogues(reader: &CatalogueReader, registry: &HarnessRegistry) {
    for listing in registry.listing() {
        if reader.installed(&listing.binary) {
            let _ = reader.read(&listing.name, &listing.binary).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::application::read_catalogue::tests::{catalogue, FakeBinaries};
    use crate::domain::registry::{CLAUDE, COPILOT, OPENCODE, PI};

    fn reader(fake: &Arc<FakeBinaries>) -> CatalogueReader {
        CatalogueReader::new(fake.clone(), || Duration::from_secs(3600))
    }

    #[tokio::test]
    async fn an_uninstalled_harness_offers_nothing_and_runs_nothing() {
        let fake = Arc::new(FakeBinaries {
            installed: vec!["claude"],
            catalogues: HashMap::from([("claude", catalogue(&["opus"], &[]))]),
            ..FakeBinaries::default()
        });
        let offers = list_offers(&reader(&fake), &HarnessRegistry::builtin()).await;
        let names: Vec<&str> = offers.iter().map(|o| o.name.as_str()).collect();
        assert_eq!(names, vec![CLAUDE, OPENCODE, COPILOT, PI]);
        assert!(offers[0].installed);
        assert_eq!(offers[0].cached.catalogue.models, vec!["opus"]);
        for offer in &offers[1..] {
            assert!(!offer.installed);
            assert_eq!(offer.cached, CachedCatalogue::default());
        }
        assert!(fake.runs().iter().all(|run| run.starts_with("claude ")));
    }

    #[tokio::test]
    async fn the_effort_axis_is_the_binarys_list_or_the_templates_hole() {
        let fake = Arc::new(FakeBinaries {
            installed: vec!["claude", "opencode", "copilot"],
            catalogues: HashMap::from([("copilot", catalogue(&[], &["low", "high"]))]),
            ..FakeBinaries::default()
        });
        let offers = list_offers(&reader(&fake), &HarnessRegistry::builtin()).await;
        let effort = |name: &str| offers.iter().find(|o| o.name == name).unwrap().has_effort;
        assert!(effort(CLAUDE), "no list, but an `{{effort}}` hole");
        assert!(!effort(OPENCODE), "no list and no hole");
        assert!(effort(COPILOT), "no hole, but the binary lists efforts");
    }

    #[tokio::test]
    async fn warming_probes_only_the_installed_binaries_once() {
        let fake = Arc::new(FakeBinaries {
            installed: vec!["pi"],
            ..FakeBinaries::default()
        });
        let reader = reader(&fake);
        warm_catalogues(&reader, &HarnessRegistry::builtin()).await;
        assert_eq!(fake.runs(), vec!["pi --version", "pi --help"]);
        list_offers(&reader, &HarnessRegistry::builtin()).await;
        assert_eq!(fake.runs().len(), 2, "the offers are served warm");
    }
}
