//! The wire shape of `set_region_max_cost`'s cap: present or not, `null`
//! included. **Pure**, like the rest of the parse.

use super::parse::CommandParseError;
use serde::Deserialize;

/// A field that is present, `null` included: `Some(Value::Null)` for an
/// explicit `null`, while `#[serde(default)]` leaves an absent one `None`.
pub(super) fn present_value<'de, D>(deserializer: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde_json::Value::deserialize(deserializer).map(Some)
}

/// The cap in dollars, or `None` for `null` (remove the cap). Absent,
/// non-numeric or non-positive is refused before any event.
pub(super) fn cost_cap(raw: Option<serde_json::Value>) -> Result<Option<f64>, CommandParseError> {
    let raw = raw.ok_or_else(|| {
        CommandParseError(
            "max_cost_usd required for set_region_max_cost (a positive amount in dollars, or null to remove the cap)"
                .into(),
        )
    })?;
    match raw {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::Number(n) => match n.as_f64() {
            Some(usd) if usd.is_finite() && usd > 0.0 => Ok(Some(usd)),
            _ => Err(CommandParseError(
                "max_cost_usd must be a positive amount in dollars".to_string(),
            )),
        },
        _ => Err(CommandParseError(
            "max_cost_usd must be a positive amount in dollars, or null".to_string(),
        )),
    }
}
