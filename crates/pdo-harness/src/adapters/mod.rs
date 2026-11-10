//! The harness adapters: what touches the environment, the filesystem and the
//! installed binaries, and the wire view of a harness's offer.

pub mod binary_path;
pub mod binary_probe;
pub mod descriptor_file;
pub mod http;
pub mod rpc_probe;
pub mod transcript_reader;
pub mod transcripts;
