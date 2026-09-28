//! The pure rules of the trigger context: a Trigger and its history, its cron
//! schedule, when it fires next, the firing decision and the plan of a tick,
//! the guard's bounds, what a configuration must satisfy. No I/O.

pub mod cron_schedule;
pub mod fire_decision;
pub mod guard;
pub mod rules;
pub mod schedule;
pub mod tick_plan;
pub mod trigger;
