//! The ports and the use cases of the stats context. The three reads ask
//! nothing; every mutation of an actor (Combine, Uncombine, the price sync)
//! asks the Autorisation d'opération before anything is written; what the
//! daemon does on its own (the boot refresh, the absorption a rename leaves
//! behind) never does (D10).

pub mod absorptions;
pub mod performance;
pub mod ports;
pub mod prices;
pub mod read_cost;
pub mod read_overview;
pub mod stats;
