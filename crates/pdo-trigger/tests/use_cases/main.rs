//! Stage 2 (ADR-0080): the use cases of the trigger context over in-memory
//! ports — every mutation asks the Autorisation d'opération first, a cron fire
//! asks as the Trigger, and what each gesture stores, announces and journals.
//! They replace the legacy crate's router `oneshot` tests of the Triggers.

mod create;
mod delete;
mod fakes;
mod fire;
mod guard;
mod pause;
mod read;
mod tick;
mod update;
