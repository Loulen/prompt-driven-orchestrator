//! What the harness use cases need from the outside world.

use std::path::PathBuf;

use crate::domain::catalogue::Catalogue;

/// The installed harness binaries (#616, ADR-0053, ADR-0055).
///
/// Implemented by `adapters::binary_probe::InstalledBinaries`; a use case test
/// injects an in-memory one.
pub trait BinaryProbe: Send + Sync {
    /// Whether `binary` resolves on the harness probe `PATH`. Never runs it.
    fn installed(&self, binary: &str) -> bool;

    /// What `<binary> --version` prints, or `None` when it answers nothing.
    /// Blocking: runs the binary.
    fn version(&self, binary: &str) -> Option<String>;

    /// The catalogue the binary offers, empty when it enumerates nothing.
    /// Blocking: runs the binary.
    fn catalogue(&self, binary: &str) -> Catalogue;
}

/// One read of the user-declared descriptor file (ADR-0045).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptorRead {
    /// Where the file lives (named in the diagnostic).
    pub path: PathBuf,
    /// Its content, `None` when it is absent or unreadable.
    pub text: Option<String>,
}

/// Where the descriptor file is read from. Implemented by
/// `adapters::descriptor_file::DescriptorFile`.
pub trait DescriptorSource: Send + Sync {
    fn read(&self) -> DescriptorRead;
}
