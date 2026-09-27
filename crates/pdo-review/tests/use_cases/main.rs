//! Stage 2 of the test pyramid (ADR-0080): the review's use cases against
//! in-memory adapters — a Run's log folded by the run context's projection, a
//! git that knows the refs a test names, the instance setting, an extension
//! host that can refuse. They replace the legacy crate's `oneshot` checks of
//! the orchestration (#926); the real git and the real log are stage 3
//! (`tests/lifecycle/`).

mod comments;
mod diff;
mod fakes;
mod gestures;
