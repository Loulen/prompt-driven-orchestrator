//! The page mounts an archive releases: the run context's `RunPages` port,
//! over the platform context (wired after the Runs).

use pdo_platform::Platform;
use pdo_run::application::ports::{BoxFuture, RunPages};

use super::Late;

/// The page mounts of the platform context.
pub(crate) struct PlatformPages(pub(crate) Late<Platform>);

impl RunPages for PlatformPages {
    fn hold(&self) -> BoxFuture<'_, Box<dyn Send>> {
        Box::pin(async move {
            match self.0.get() {
                Ok(platform) => {
                    Box::new(platform.release_run_pages().hold_owned().await) as Box<dyn Send>
                }
                Err(_) => Box::new(()) as Box<dyn Send>,
            }
        })
    }

    fn release<'a>(&'a self, run_id: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.0
                .get()?
                .release_run_pages()
                .execute(run_id)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string())
        })
    }
}
