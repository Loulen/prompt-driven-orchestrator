//! What the core gives an extension when the daemon starts: its
//! [`ExtensionContext`].

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use crate::{Host, NeutralHost};

/// What the core gives an extension when the daemon starts.
#[derive(Clone)]
#[non_exhaustive]
pub struct ExtensionContext {
    /// The extension's own data folder, `~/.pdo/extensions/<id>/`, created
    /// before [`Extension::start`](crate::Extension::start) is called: its database, its files. Nothing
    /// else of the core writes there.
    pub data_dir: PathBuf,
    /// The core's mechanisms, for the extension's own mutations: the
    /// Autorisation d'opération of every extension, the events, the
    /// attributes of the other extensions. Keep it: it serves as long as the
    /// daemon runs.
    pub host: Arc<dyn Host>,
}

impl ExtensionContext {
    /// The context of the data folder `data_dir`, with the [`NeutralHost`]
    /// (everything allowed, events go nowhere, no attribute).
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            host: Arc::new(NeutralHost),
        }
    }

    /// The same context, with `host`.
    pub fn with_host(mut self, host: Arc<dyn Host>) -> Self {
        self.host = host;
        self
    }
}

impl fmt::Debug for ExtensionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExtensionContext")
            .field("data_dir", &self.data_dir)
            .finish_non_exhaustive()
    }
}

/// Two contexts are equal when they name the same folder and share the same
/// host.
impl PartialEq for ExtensionContext {
    fn eq(&self, other: &Self) -> bool {
        self.data_dir == other.data_dir
            && std::ptr::addr_eq(Arc::as_ptr(&self.host), Arc::as_ptr(&other.host))
    }
}

impl Eq for ExtensionContext {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_context_carries_the_neutral_host_until_given_one() {
        let context = ExtensionContext::new("/tmp/x");
        assert_eq!(context, context.clone());
        let other = ExtensionContext::new("/tmp/x").with_host(Arc::new(NeutralHost));
        assert_ne!(context, other, "another host");
        assert!(format!("{context:?}").contains("/tmp/x"));
    }
}
