//! Masks for the temporary snapshot suite: everything a run of the suite cannot
//! reproduce byte for byte is replaced by a stable placeholder.
//!
//! - fixed strings (the daemon's repo root, address, port, `$HOME`) → `[repo]`,
//!   `[addr]`, `[port]`, `[home]`;
//! - timestamps → `[ts]`, bare dates → `[date]`;
//! - run ids, UUIDs, SHAs, short hex ids, temp dirs → a placeholder **numbered by
//!   first appearance** (`[run-1]`, `[sha-2]`): the value is gone, the relation
//!   between two fields carrying the same value stays frozen;
//! - numbers under a masked key (durations, ages, sizes…) → `"[n]"`;
//! - what describes the machine rather than the contract (installed harnesses,
//!   docker, the environment tier of the settings) → a named placeholder;
//! - the stray events the daemon emits on a timing it does not own → dropped.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

/// Keys whose numbers depend on the wall clock or the machine.
const CLOCK_KEYS: &[&str] = &[
    "age_secs",
    "approx_disk_bytes",
    "duration_ms",
    "duration_secs",
    "elapsed_ms",
    "elapsed_secs",
    "last_tick_age_secs",
    "pid",
    "wall_ms",
];

/// Keys whose whole value describes the machine, not the contract: which
/// harnesses are installed (and their model lists), whether docker answers, what
/// the process environment carries. Replaced by a placeholder, key kept.
const MACHINE_KEYS: &[(&str, &str)] = &[
    ("harness_descriptors", "[machine: harness catalogue]"),
    ("sandbox_docker", "[machine: docker probe]"),
    ("env", "[machine: environment]"),
];

/// Event kinds the daemon emits on a timing it does not own. The pipeline file
/// watcher can report a Run's own `pipeline.yaml` as modified, or not, depending
/// on when its debounce fires — a known stray the projection already ignores
/// (`event_log.rs`, #221). Dropped wherever an event list carries it.
pub const STRAY_EVENT_KINDS: &[&str] = &["pipeline_modified"];

pub fn is_stray_event(value: &Value) -> bool {
    value["kind"]
        .as_str()
        .is_some_and(|kind| STRAY_EVENT_KINDS.contains(&kind))
}

#[derive(Default)]
pub struct Masker {
    fixed: Vec<(String, String)>,
    numbered: BTreeMap<String, String>,
    counters: BTreeMap<&'static str, usize>,
    /// Keys whose whole numeric subtree is masked (scenario-specific).
    pub masked_keys: BTreeSet<String>,
}

impl Masker {
    /// Replace `from` by `to` verbatim (longest fixed strings first).
    pub fn fixed(&mut self, from: impl Into<String>, to: impl Into<String>) {
        let from = from.into();
        if from.is_empty() {
            return;
        }
        self.fixed.push((from, to.into()));
        self.fixed
            .sort_by_key(|(from, _)| std::cmp::Reverse(from.len()));
    }

    /// Number `token` under `kind` (`[kind-N]`), whatever pattern it has.
    pub fn alias(&mut self, token: &str, kind: &'static str) -> String {
        if let Some(existing) = self.numbered.get(token) {
            return existing.clone();
        }
        let n = self.counters.entry(kind).or_insert(0);
        *n += 1;
        let placeholder = format!("[{kind}-{n}]");
        self.numbered.insert(token.to_string(), placeholder.clone());
        placeholder
    }

    pub fn value(&mut self, value: &Value) -> Value {
        self.value_under(value, false)
    }

    fn value_under(&mut self, value: &Value, masked: bool) -> Value {
        match value {
            Value::String(s) => Value::String(self.text(s)),
            Value::Number(_) if masked => Value::String("[n]".into()),
            Value::Array(items) => Value::Array(
                items
                    .iter()
                    .filter(|v| !is_stray_event(v))
                    .map(|v| self.value_under(v, masked))
                    .collect(),
            ),
            Value::Object(map) => {
                let mut out = serde_json::Map::new();
                for (key, v) in map {
                    if let Some((_, placeholder)) = MACHINE_KEYS.iter().find(|(k, _)| k == key) {
                        out.insert(key.clone(), Value::String((*placeholder).into()));
                        continue;
                    }
                    let under = masked
                        || CLOCK_KEYS.contains(&key.as_str())
                        || self.masked_keys.contains(key);
                    out.insert(self.text(key), self.value_under(v, under));
                }
                Value::Object(out)
            }
            other => other.clone(),
        }
    }

    pub fn text(&mut self, s: &str) -> String {
        let mut out = s.to_string();
        for (from, to) in &self.fixed {
            out = out.replace(from.as_str(), to);
        }
        // Tokens already numbered first: a run id also lives inside `pdo/run-…`,
        // `pdo-mgr-…`, paths.
        let mut known: Vec<(&String, &String)> = self.numbered.iter().collect();
        known.sort_by_key(|(token, _)| std::cmp::Reverse(token.len()));
        for (token, placeholder) in known {
            out = out.replace(token.as_str(), placeholder);
        }
        self.scan(&out)
    }

    /// Discover and replace the patterned tokens of `s`.
    fn scan(&mut self, s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::with_capacity(s.len());
        let mut i = 0;
        while i < chars.len() {
            let before = i.checked_sub(1).map(|j| chars[j]);
            let boundary = before.is_none_or(|c| !c.is_ascii_alphanumeric());
            if boundary {
                if let Some(len) = timestamp_len(&chars[i..]) {
                    out.push_str("[ts]");
                    i += len;
                    continue;
                }
                if let Some(len) = date_len(&chars[i..]) {
                    out.push_str("[date]");
                    i += len;
                    continue;
                }
                if let Some((len, kind)) = token_at(&chars[i..]) {
                    let token: String = chars[i..i + len].iter().collect();
                    let placeholder = match kind {
                        "short" => self.short_hex(&token),
                        kind => self.alias(&token, kind),
                    };
                    out.push_str(&placeholder);
                    i += len;
                    continue;
                }
            }
            out.push(chars[i]);
            i += 1;
        }
        out
    }

    /// A short SHA is numbered like the full SHA it abbreviates, when known.
    fn short_hex(&mut self, token: &str) -> String {
        let full = self
            .numbered
            .iter()
            .find(|(full, p)| p.starts_with("[sha-") && full.starts_with(token))
            .map(|(_, p)| p.clone());
        match full {
            Some(p) => format!("{}:short]", p.trim_end_matches(']')),
            None => self.alias(token, "hex"),
        }
    }
}

fn is_hex(c: char) -> bool {
    c.is_ascii_digit() || ('a'..='f').contains(&c)
}

fn digits(chars: &[char], at: usize, n: usize) -> bool {
    chars.len() >= at + n && chars[at..at + n].iter().all(char::is_ascii_digit)
}

/// `YYYY-MM-DDTHH:MM:SS[.frac][Z|±HH:MM]`.
fn timestamp_len(c: &[char]) -> Option<usize> {
    let shape = digits(c, 0, 4)
        && c.get(4) == Some(&'-')
        && digits(c, 5, 2)
        && c.get(7) == Some(&'-')
        && digits(c, 8, 2)
        && matches!(c.get(10), Some('T') | Some(' '))
        && digits(c, 11, 2)
        && c.get(13) == Some(&':')
        && digits(c, 14, 2)
        && c.get(16) == Some(&':')
        && digits(c, 17, 2);
    if !shape {
        return None;
    }
    let mut len = 19;
    if c.get(len) == Some(&'.') {
        len += 1;
        while c.get(len).is_some_and(char::is_ascii_digit) {
            len += 1;
        }
    }
    if c.get(len) == Some(&'Z') {
        len += 1;
    } else if matches!(c.get(len), Some('+') | Some('-'))
        && digits(c, len + 1, 2)
        && c.get(len + 3) == Some(&':')
        && digits(c, len + 4, 2)
    {
        len += 6;
    }
    Some(len)
}

/// A bare `YYYY-MM-DD` (a Stats bucket, a changelog date).
fn date_len(c: &[char]) -> Option<usize> {
    let shape = digits(c, 0, 4)
        && c.get(4) == Some(&'-')
        && digits(c, 5, 2)
        && c.get(7) == Some(&'-')
        && digits(c, 8, 2)
        && c.get(10).is_none_or(|x| !x.is_ascii_alphanumeric());
    shape.then_some(10)
}

/// A run id (`20260926-161054-5144f36`), a UUID, a 40-hex SHA, a temp dir name
/// (`.tmpAbC123`), or a short hex id (7–12 hex, digits and letters mixed).
fn token_at(c: &[char]) -> Option<(usize, &'static str)> {
    let ends_at = |len: usize| c.get(len).is_none_or(|x| !x.is_ascii_alphanumeric());
    // Run / Trigger-fire style id: 8 digits, 6 digits, 7 hex.
    if digits(c, 0, 8)
        && c.get(8) == Some(&'-')
        && digits(c, 9, 6)
        && c.get(15) == Some(&'-')
        && c.len() >= 23
        && c[16..23].iter().all(|x| is_hex(*x))
        && ends_at(23)
    {
        return Some((23, "run"));
    }
    // UUID.
    let groups = [8, 4, 4, 4, 12];
    let mut at = 0;
    let mut uuid = true;
    for (g, n) in groups.iter().enumerate() {
        if c.len() < at + n || !c[at..at + n].iter().all(|x| is_hex(*x)) {
            uuid = false;
            break;
        }
        at += n;
        if g < 4 {
            if c.get(at) != Some(&'-') {
                uuid = false;
                break;
            }
            at += 1;
        }
    }
    if uuid && ends_at(36) {
        return Some((36, "uuid"));
    }
    // Temp dir name made by `tempfile` (`.tmp` + 6 alphanumerics).
    if c.len() >= 10
        && c[..4] == ['.', 't', 'm', 'p']
        && c[4..10].iter().all(char::is_ascii_alphanumeric)
        && ends_at(10)
    {
        return Some((10, "tmp"));
    }
    let hex_len = c.iter().take_while(|x| is_hex(**x)).count();
    if hex_len == 40 && ends_at(40) {
        return Some((40, "sha"));
    }
    if (7..=12).contains(&hex_len) && ends_at(hex_len) {
        let run = &c[..hex_len];
        if run.iter().any(char::is_ascii_digit) && run.iter().any(|x| x.is_ascii_alphabetic()) {
            return Some((hex_len, "short"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_are_stable_and_keep_relations() {
        let mut m = Masker::default();
        m.fixed("/tmp/.tmpAbc123", "[repo]");
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let v = serde_json::json!({
            "run_id": "20260926-161054-5144f36",
            "branch": "pdo/run-20260926-161054-5144f36",
            "ts": "2026-09-26T16:10:54.123456Z",
            "path": "/tmp/.tmpAbc123/.pdo/runs",
            "sha": sha,
            "short": "0123456789ab",
            "session": "4c8e2a1e-6f5c-4a8b-9d9e-0e1f2a3b4c5d",
            "duration_ms": 1234,
            "count": 3,
            "word": "added",
        });
        assert_eq!(
            m.value(&v),
            serde_json::json!({
                "run_id": "[run-1]",
                "branch": "pdo/run-[run-1]",
                "ts": "[ts]",
                "path": "[repo]/.pdo/runs",
                "sha": "[sha-1]",
                "short": "[sha-1:short]",
                "session": "[uuid-1]",
                "duration_ms": "[n]",
                "count": 3,
                "word": "added",
            })
        );
    }
}
