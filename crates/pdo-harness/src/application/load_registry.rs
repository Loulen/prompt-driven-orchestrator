//! Load the effective harness registry: the embedded floor merged with the
//! user-declared descriptor file, by name (ADR-0045).

use std::sync::Mutex;

use tracing::warn;

use crate::application::ports::DescriptorSource;
use crate::domain::registry::HarnessRegistry;

/// The registry for one read. Absent or unreadable file → the floor, silent.
/// Unparseable, or a refused row → the floor for that key plus a diagnostic,
/// logged AT MOST ONCE per distinct message, so a polled spawn/settings path
/// cannot flood the log. Never writes, never seeds.
pub fn load_registry(source: &dyn DescriptorSource) -> HarnessRegistry {
    let read = source.read();
    let registry = HarnessRegistry::from_disk_tier(read.path, read.text.as_deref());
    if let Some(msg) = registry.diagnostic() {
        if LAST_WARNED.first_time(&msg) {
            warn!("{msg}");
        }
    }
    registry
}

/// The last diagnostic the daemon said.
static LAST_WARNED: LastSaid = LastSaid(Mutex::new(None));

/// The fingerprint of the last message said. Same posture as
/// `price_table::PriceTable::warn_once` (a loader called once per request must
/// not log per request).
struct LastSaid(Mutex<Option<u64>>);

impl LastSaid {
    /// Whether `msg` differs from the last one said — a *differently* bad file
    /// hashes differently and is said again.
    fn first_time(&self, msg: &str) -> bool {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        std::hash::Hash::hash(msg, &mut h);
        let fingerprint = std::hash::Hasher::finish(&h);
        let mut guard = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if *guard == Some(fingerprint) {
            return false;
        }
        *guard = Some(fingerprint);
        true
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::application::ports::DescriptorRead;
    use crate::domain::registry::{HarnessSource, CLAUDE};

    /// A descriptor file held in memory.
    struct InMemory(Option<&'static str>);

    impl DescriptorSource for InMemory {
        fn read(&self) -> DescriptorRead {
            DescriptorRead {
                path: PathBuf::from("/home/u/.pdo/harnesses/descriptors.yaml"),
                text: self.0.map(str::to_string),
            }
        }
    }

    #[test]
    fn an_absent_file_is_the_floor_and_says_nothing() {
        let registry = load_registry(&InMemory(None));
        assert_eq!(registry.names(), HarnessRegistry::builtin().names());
        assert_eq!(registry.diagnostic(), None);
    }

    #[test]
    fn a_declared_harness_is_merged_by_name_over_the_floor() {
        let registry = load_registry(&InMemory(Some(
            "harnesses:\n  mine:\n    binary: mine\n    launch: [\"exec\", \"mine\", \"{prompt}\"]\n",
        )));
        let listing = registry.listing();
        let mine = listing.iter().find(|h| h.name == "mine").unwrap();
        assert_eq!(mine.source, HarnessSource::Descriptor);
        assert!(registry.resolve(CLAUDE).is_some());
    }

    #[test]
    fn a_refused_row_is_named_with_the_file_it_came_from() {
        let registry = load_registry(&InMemory(Some(
            "harnesses:\n  claude:\n    binary: claude\n    launch: [\"claude\"]\n",
        )));
        let diagnostic = registry.diagnostic().unwrap();
        assert!(diagnostic.contains("/home/u/.pdo/harnesses/descriptors.yaml"));
        assert!(diagnostic.contains("`claude`"));
        assert_eq!(
            registry.resolve(CLAUDE).unwrap(),
            crate::domain::registry::claude()
        );
    }

    #[test]
    fn the_same_diagnostic_is_said_once_and_a_new_one_again() {
        let said = LastSaid(Mutex::new(None));
        assert!(said.first_time("A"));
        assert!(!said.first_time("A"));
        assert!(said.first_time("B"));
        assert!(said.first_time("A"));
    }
}
