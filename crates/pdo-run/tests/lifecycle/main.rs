//! Stage 3 of the test pyramid (ADR-0080): the run context over its real
//! adapters — the SQLite event store, git worktrees in a temporary repository,
//! tmux on a socket of its own — driven through its router or its use cases.
//! Moved from the legacy crate's in-crate tests with the code they test (#924).

mod buttons;
mod commands;
mod completion;
mod harness;
mod helpers;
mod reads;
mod spawn;
mod sweeps;
mod terminals;
