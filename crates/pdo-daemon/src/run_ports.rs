//! The port of the run context that what is still legacy implements: the page
//! mounts an archive releases (the platform context, reached through the
//! daemon's state).

use std::sync::Weak;

use pdo_run::application::ports::{BoxFuture, RunPages};

use crate::AppState;

/// The page mounts of the platform context, reached through the daemon's
/// state.
pub(crate) struct LegacyRunPages {
    pub(crate) state: Weak<AppState>,
}

impl RunPages for LegacyRunPages {
    fn hold(&self) -> BoxFuture<'_, Box<dyn Send>> {
        Box::pin(async move {
            match self.state.upgrade() {
                Some(state) => {
                    Box::new(state.platform.release_run_pages().hold_owned().await) as Box<dyn Send>
                }
                None => Box::new(()) as Box<dyn Send>,
            }
        })
    }

    fn release<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let Some(state) = self.state.upgrade() else {
                return Err("the daemon is shutting down".to_string());
            };
            state
                .platform
                .release_run_pages()
                .execute(run_id)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
}
