//! A Run's cost and tokens, as the telemetry records them once the Run ends:
//! one slice per harness × model the run context could read.

/// The tokens of one slice, by kind.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
}

impl Tokens {
    /// Every kind with its count, as the `pdo.token.type` attribute names it.
    pub fn by_type(&self) -> [(&'static str, u64); 4] {
        [
            ("input", self.input),
            ("output", self.output),
            ("cache_read", self.cache_read),
            ("cache_write", self.cache_write),
        ]
    }
}

/// One harness × model share of a Run's cost.
#[derive(Debug, Clone, PartialEq)]
pub struct CostSlice {
    pub harness: String,
    pub model: Option<String>,
    /// In US dollars; `None` when the harness has no cost source.
    pub usd: Option<f64>,
    /// `None` when the source does not count tokens.
    pub tokens: Option<Tokens>,
}
