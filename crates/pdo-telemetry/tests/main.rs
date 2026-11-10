//! The telemetry's use cases over in-memory exporters (ADR-0090): off by
//! default, on and off again without a restart, the Runs' measures attached to
//! the Projet, the repository and git, the Run and node ids on the traces only,
//! the Actor only when included; `/metrics` beside the export, same measures. And, over the real OTLP exporter, a collector that does
//! not answer: said in the status, without effect on the measures.

mod fakes;
mod otlp;
mod prometheus;
mod runs;
mod switch;
