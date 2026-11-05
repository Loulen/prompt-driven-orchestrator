//! The structural checks of the repository: the tests under `tests/` read the
//! source tree as text, never the compiled product, so they run before any long
//! suite (`make fast`). A check that needs the code of a product crate stays out
//! of this crate.
//!
//! The library holds what another test binary reuses: the router reader of the
//! route coverage ([`routes`]), which the daemon's mutation-route test reads too,
//! and the CLI parity built on it ([`cli_parity`]).

use std::path::{Path, PathBuf};

pub mod cli_parity;
pub mod routes;

/// The root of the repository.
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the workspace root exists")
}
