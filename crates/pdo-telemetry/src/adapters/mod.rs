//! The telemetry's adapters: the OTLP exporters, the `/metrics` reader, the
//! routes and the request layer, the environment tier and the settings page's
//! disclosure, git and the host name.

pub mod env;
pub mod git;
pub mod host;
pub mod http;
pub mod instance_defaults;
pub mod otlp;
pub mod prometheus;
