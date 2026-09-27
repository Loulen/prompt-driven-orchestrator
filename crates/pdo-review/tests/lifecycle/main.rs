//! Stage 3 of the test pyramid (ADR-0080): the review context over its real
//! adapters — the run context's SQLite log, git in temporary repositories —
//! driven through its router. Moved from the legacy crate's in-crate tests
//! with the code they test (#926).

mod comments;
mod diff;
mod harness;
mod helpers;
