//! The `harness` bounded context (ADR-0079): which agentic harness runs a node,
//! how it is launched, what PDO knows how to read from it, and what model and
//! effort it offers.
//!
//! - [`domain`]: the descriptors and their registry, the argv templates, the
//!   per-harness capabilities and the parsers of each harness's own artefacts
//!   (transcripts, turn state, reported cost), the catalogue parsers, and the
//!   model × effort resolution.
//! - [`application`]: the ports ([`application::ports`]) and the use cases that
//!   read the installed binaries and the descriptor file through them.
//! - [`adapters`]: the `PATH` lookup, the binary probe that runs a harness for
//!   its catalogue, the descriptor file, the transcript stores on disk, and the
//!   wire view of a harness's offer.
//!
//! Other contexts (session, sandbox, pipeline, run, stats) depend on this one,
//! never the reverse: this crate depends on the kernel only.

pub mod adapters;
pub mod application;
pub mod domain;

pub use domain::{
    agent_choice, argv, catalogue, claude_transcript, context_peak, copilot_journal, pi_session,
    probes, registry, resolver, steering, support, transcript,
};
