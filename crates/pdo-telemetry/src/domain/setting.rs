//! The telemetry setting (ADR-0090 §1-2, §4): the OTLP export (its switch, off
//! by default, the collector's address and the headers sent with every export),
//! the `/metrics` switch, off by default, and whether the Actor is added to the
//! measures, off by default. The headers' values are secrets (an API key):
//! stored on the daemon, never shown again.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// The two signals the export pushes; logs are never exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Metrics,
    Traces,
}

impl Signal {
    /// The OTLP/HTTP path the collector receives the signal on.
    pub fn path(self) -> &'static str {
        match self {
            Signal::Metrics => "/v1/metrics",
            Signal::Traces => "/v1/traces",
        }
    }
}

/// The whole setting as the daemon applies it, every tier resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TelemetrySetting {
    pub otlp: OtlpSetting,
    /// Whether `/metrics` answers.
    pub prometheus: bool,
    /// Whether who launched a Run, and who answered it, goes with its measures.
    pub include_actor: bool,
}

impl TelemetrySetting {
    /// Where the measures go: the channels that are on.
    pub fn channels(&self) -> Channels {
        Channels {
            otlp: self.otlp.target(),
            prometheus: self.prometheus,
        }
    }
}

/// The channels the measures go through. With none, nothing is measured.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Channels {
    /// The collector the OTLP export pushes to.
    pub otlp: Option<ExportTarget>,
    /// `/metrics`, read by whoever reaches the instance.
    pub prometheus: bool,
}

impl Channels {
    pub fn any(&self) -> bool {
        self.otlp.is_some() || self.prometheus
    }
}

/// The OTLP export setting, every tier resolved.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OtlpSetting {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub headers: BTreeMap<String, String>,
}

impl OtlpSetting {
    /// Where to export, or `None` when nothing may leave: the switch is off, or
    /// no collector is named.
    pub fn target(&self) -> Option<ExportTarget> {
        let endpoint = self.endpoint.as_deref().map(str::trim)?;
        (self.enabled && !endpoint.is_empty()).then(|| ExportTarget {
            endpoint: endpoint.trim_end_matches('/').to_string(),
            headers: self.headers.clone(),
        })
    }
}

/// A collector to export to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportTarget {
    /// The base address, without a trailing `/`.
    pub endpoint: String,
    pub headers: BTreeMap<String, String>,
}

impl ExportTarget {
    /// The full URL of `signal`: the base address plus the OTLP path.
    pub fn url(&self, signal: Signal) -> String {
        format!("{}{}", self.endpoint, signal.path())
    }
}

/// The refusal of an address the exporter could not post to.
pub fn check_endpoint(raw: &str) -> Result<(), String> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(());
    }
    let rest = raw
        .strip_prefix("http://")
        .or_else(|| raw.strip_prefix("https://"))
        .ok_or_else(|| {
            format!("telemetry_otlp_endpoint: `{raw}` must start with http:// or https://")
        })?;
    let host = rest.split(['/', '?', '#']).next().unwrap_or("");
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err(format!("telemetry_otlp_endpoint: `{raw}` names no host"));
    }
    Ok(())
}

/// One header of an edit. `value: None` keeps the value stored under the same
/// name: the page never has it, so re-saving the page must not erase it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export, optional_fields = nullable)]
pub struct OtlpHeaderEdit {
    pub name: String,
    pub value: Option<String>,
}

/// One header as the page shows it: its name, never its value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export)]
pub struct OtlpHeaderView {
    pub name: String,
    pub set: bool,
}

/// The stored headers, named only.
pub fn header_views(headers: &BTreeMap<String, String>) -> Vec<OtlpHeaderView> {
    headers
        .keys()
        .map(|name| OtlpHeaderView {
            name: name.clone(),
            set: true,
        })
        .collect()
}

/// The headers after `edits`, which replace the stored list wholesale. A header
/// without a value keeps the one stored under its name (case-insensitively); a
/// new header without a value is refused, as is an invalid or repeated name.
pub fn merge_headers(
    stored: &BTreeMap<String, String>,
    edits: Vec<OtlpHeaderEdit>,
) -> Result<BTreeMap<String, String>, String> {
    let mut merged = BTreeMap::new();
    for edit in edits {
        let name = edit.name.trim().to_string();
        check_header_name(&name)?;
        if merged
            .keys()
            .any(|n: &String| n.eq_ignore_ascii_case(&name))
        {
            return Err(format!("telemetry_otlp_headers: `{name}` is given twice"));
        }
        let value = match edit.value {
            Some(value) => value,
            None => stored
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
                .map(|(_, v)| v.clone())
                .ok_or_else(|| format!("telemetry_otlp_headers: `{name}` has no value"))?,
        };
        if value.contains(['\r', '\n']) {
            return Err(format!(
                "telemetry_otlp_headers: the value of `{name}` spans lines"
            ));
        }
        merged.insert(name, value);
    }
    Ok(merged)
}

/// An HTTP header name: a non-empty token (RFC 9110 §5.6.2).
fn check_header_name(name: &str) -> Result<(), String> {
    let token = |c: char| c.is_ascii_alphanumeric() || "!#$%&'*+-.^_`|~".contains(c);
    if name.is_empty() || !name.chars().all(token) {
        return Err(format!(
            "telemetry_otlp_headers: `{name}` is not a valid header name"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn headers(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn edit(name: &str, value: Option<&str>) -> OtlpHeaderEdit {
        OtlpHeaderEdit {
            name: name.into(),
            value: value.map(Into::into),
        }
    }

    #[test]
    fn nothing_leaves_while_the_switch_is_off_or_no_collector_is_named() {
        let off = OtlpSetting {
            enabled: false,
            endpoint: Some("http://collector:4318".into()),
            headers: BTreeMap::new(),
        };
        assert_eq!(off.target(), None);
        let unnamed = OtlpSetting {
            enabled: true,
            endpoint: Some("  ".into()),
            headers: BTreeMap::new(),
        };
        assert_eq!(unnamed.target(), None);
        assert_eq!(OtlpSetting::default().target(), None);
    }

    #[test]
    fn each_channel_has_its_own_switch() {
        let collector = OtlpSetting {
            enabled: true,
            endpoint: Some("http://collector:4318".into()),
            headers: BTreeMap::new(),
        };
        let none = TelemetrySetting::default().channels();
        assert!(!none.any());
        let prometheus_only = TelemetrySetting {
            prometheus: true,
            ..Default::default()
        }
        .channels();
        assert!(prometheus_only.any());
        assert_eq!(prometheus_only.otlp, None);
        let otlp_only = TelemetrySetting {
            otlp: collector,
            ..Default::default()
        }
        .channels();
        assert!(otlp_only.any() && !otlp_only.prometheus);
        let actor_alone = TelemetrySetting {
            include_actor: true,
            ..Default::default()
        };
        assert!(!actor_alone.channels().any(), "the Actor opens no channel");
    }

    #[test]
    fn the_signal_path_is_appended_to_the_address() {
        let on = OtlpSetting {
            enabled: true,
            endpoint: Some("https://otel.example.org:4318/".into()),
            headers: BTreeMap::new(),
        };
        let target = on.target().unwrap();
        assert_eq!(
            target.url(Signal::Metrics),
            "https://otel.example.org:4318/v1/metrics"
        );
        assert_eq!(
            target.url(Signal::Traces),
            "https://otel.example.org:4318/v1/traces"
        );
    }

    #[test]
    fn an_address_must_be_http_with_a_host() {
        assert!(check_endpoint("http://127.0.0.1:4318").is_ok());
        assert!(check_endpoint("https://otel.example.org").is_ok());
        assert!(check_endpoint("").is_ok(), "empty clears the address");
        assert!(check_endpoint("otel.example.org:4318").is_err());
        assert!(check_endpoint("http:///v1").is_err());
    }

    #[test]
    fn a_header_without_value_keeps_the_stored_one() {
        let stored = headers(&[("Authorization", "Bearer s3cret")]);
        let merged = merge_headers(
            &stored,
            vec![edit("authorization", None), edit("X-Team", Some("core"))],
        )
        .unwrap();
        assert_eq!(
            merged,
            headers(&[("X-Team", "core"), ("authorization", "Bearer s3cret")])
        );
    }

    #[test]
    fn the_edit_replaces_the_list_so_an_absent_header_is_removed() {
        let stored = headers(&[("Authorization", "Bearer s3cret")]);
        assert_eq!(merge_headers(&stored, Vec::new()).unwrap(), BTreeMap::new());
    }

    #[test]
    fn a_new_header_needs_a_value_and_a_valid_unique_name() {
        let stored = BTreeMap::new();
        assert!(merge_headers(&stored, vec![edit("X-Key", None)]).is_err());
        assert!(merge_headers(&stored, vec![edit("Bad Name", Some("v"))]).is_err());
        assert!(merge_headers(&stored, vec![edit("X-Key", Some("a\r\nb"))]).is_err());
        assert!(merge_headers(
            &stored,
            vec![edit("X-Key", Some("a")), edit("x-key", Some("b"))]
        )
        .is_err());
    }

    #[test]
    fn the_page_sees_the_names_never_the_values() {
        let views = header_views(&headers(&[("Authorization", "Bearer s3cret")]));
        assert_eq!(
            serde_json::to_value(views).unwrap(),
            serde_json::json!([{ "name": "Authorization", "set": true }])
        );
    }
}
