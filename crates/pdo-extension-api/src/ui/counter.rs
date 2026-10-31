//! What the counter of an icon of `topbar/end`, or of a global Action of
//! `palette`, says: the answer of its contribution's `counter` path.

use serde::Serialize;

/// What the counter of an icon says to one Actor (0.19): what its
/// contribution's `counter` path answers. A count of `0` shows nothing.
///
/// On the wire: `{"count": 3, "accent": true, "detail": "3 sessions, 1 waiting"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[non_exhaustive]
pub struct PageCounter {
    pub count: u32,
    /// Drawn in the accent colour: something waits for the Actor.
    #[serde(default)]
    pub accent: bool,
    /// One line for the icon's tooltip, after the page's label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub detail: Option<String>,
}

impl PageCounter {
    /// `count`, neutral, without detail.
    pub fn new(count: u32) -> Self {
        Self {
            count,
            accent: false,
            detail: None,
        }
    }

    /// The same counter, in the accent colour when `accent`.
    pub fn accented(mut self, accent: bool) -> Self {
        self.accent = accent;
        self
    }

    /// The same counter, with `detail` in the tooltip.
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}
