//! The pure rules of the stats context: the folds of the three sections, the
//! R-7 distribution, the absorptions and their resolution, the price sync's
//! normalisation and diff. No I/O.

pub mod absorption;
pub mod bucket;
pub mod cost;
pub mod distribution;
pub mod overview;
pub mod performance;
pub mod price_sync;
pub mod provenance;
