//! The pure rules of the platform context. No I/O here: storage, processes,
//! HTTP and the environment are reached through the ports of `application`.

pub mod attempt;
pub mod audit;
pub mod changelog;
pub mod install;
pub mod page_mount;
pub mod release;
pub mod release_channel;
pub mod service_health;
pub mod service_unit;
pub mod update_check;
pub mod update_plan;
pub mod update_status;
