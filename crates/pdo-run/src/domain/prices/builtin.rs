//! The prices PDO ships and its no-cost sentinel.

///
/// `claude-sonnet-5` is graved at its **post-intro** $3/$15, not the dated intro
/// $2/$10: the `const` cannot be dated, the intro expires 2026-08-31, and a sync
/// corrects the ~0.5 % pre-cutover drift.
pub const PRICES: &[(&str, f64, f64)] = &[
    ("claude-opus-4-8", 5.0, 25.0),
    ("claude-opus-4-7", 5.0, 25.0),
    ("claude-opus-4-6", 5.0, 25.0),
    ("claude-opus-4-5", 5.0, 25.0),
    ("claude-opus-4-1", 15.0, 75.0),
    ("claude-opus-4-0", 15.0, 75.0),
    ("claude-sonnet-4-6", 3.0, 15.0),
    ("claude-sonnet-4-5", 3.0, 15.0),
    ("claude-sonnet-4-0", 3.0, 15.0),
    ("claude-haiku-4-5", 1.0, 5.0),
    ("claude-3-5-haiku", 0.80, 4.0),
    ("claude-opus-5", 5.0, 25.0),
    ("claude-sonnet-5", 3.0, 15.0),
    ("claude-fable-5", 10.0, 50.0),
];

/// Claude Code's local/no-cost sentinel. Priced $0 **above every tier** so it can
/// never flip `partial` — ORDER IS LOAD-BEARING (see [`PriceTable::price_for`]).
pub const SYNTHETIC: &str = "<synthetic>";
