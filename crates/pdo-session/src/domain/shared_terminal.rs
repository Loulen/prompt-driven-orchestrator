//! Shared terminal (#867, ADR-0075): who pilots a tmux session when several
//! **postes** (browsers) show it — the roles and the frames that announce them.
//! The presence registry that decides them is `application::shared_terminal`.
//!
//! The registry keeps, per tmux session, the live PTY-bridge connections (their
//! poste, their arrival order, their last size) and the pilot poste. It decides
//! roles; the bridge applies them:
//!
//! - **solo** while a single poste is connected (one or several tabs): no role is
//!   visible, every connection types and weighs on the size, as before;
//! - otherwise one **pilot** (the first arrived, or whoever keeps the hand) and
//!   **spectators**. A spectator's tmux client carries `ignore-size`, flipped live
//!   with `refresh-client -f` (never a re-attach), and the bridge drops its input
//!   frames — read-only is held by the bridge, not by tmux's `read-only` flag.
//!
//! A spectator **takes control** with a `take_control` frame (#870): its poste
//! becomes the pilot, the former pilot a spectator, and the same `settle` flips
//! the `ignore-size` of both postes' clients live. A take-over from the pilot or
//! from a solo terminal changes nothing.
//!
//! A connection whose Actor may read the session but not attach to it
//! (`session` × `read` without `session` × `attach`) is **read-only**: it never
//! drives, never pilots, never takes control, and its frames carry the
//! sentence of that refusal (`read_only`). Alone, or among read-only postes
//! only, the first arrived of them sizes the terminal (`solo`, read-only); the
//! others watch its grid.
//!
//! Every change is pushed to each connection as a `role` text frame. A frame is
//! only sent when it differs from the last one that connection received, so a
//! spectator hears about a pilot resize and nothing else.

use serde::Serialize;
use ts_rs::TS;

/// A connection's handle in the registry.
pub type ConnId = u64;

/// The role of one connection on its terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Solo,
    Pilot,
    Spectator,
}

/// Columns × rows of a PTY.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SharedTerminalSize")]
pub struct TermSize {
    pub cols: u16,
    pub rows: u16,
}

/// The size a fresh bridge PTY opens at, before the browser's first `resize`.
pub const INITIAL_SIZE: TermSize = TermSize { cols: 80, rows: 24 };

/// The server → client `role` frame on the terminal socket's Text channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, TS)]
#[ts(export, rename = "SharedTerminalRole")]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum RoleMsg {
    Solo {
        /// See [`RoleMsg::read_only`].
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        read_only: Option<String>,
    },
    Pilot {
        /// Other postes watching this terminal.
        spectators: usize,
    },
    Spectator {
        /// The pilot's grid: the spectator renders exactly this many cells.
        pilot: TermSize,
        /// See [`RoleMsg::read_only`].
        #[serde(skip_serializing_if = "Option::is_none")]
        #[ts(optional)]
        read_only: Option<String>,
    },
}

impl RoleMsg {
    /// A solo terminal whose connection may drive it.
    pub const SOLO: RoleMsg = RoleMsg::Solo { read_only: None };

    pub fn role(&self) -> Role {
        match self {
            RoleMsg::Solo { .. } => Role::Solo,
            RoleMsg::Pilot { .. } => Role::Pilot,
            RoleMsg::Spectator { .. } => Role::Spectator,
        }
    }

    /// The sentence of the refusal that keeps this connection from ever
    /// taking the hand (no `session` × `attach`): it watches, read-only,
    /// whatever happens to the others. `None` for a connection that may drive.
    pub fn read_only(&self) -> Option<&str> {
        match self {
            RoleMsg::Solo { read_only } | RoleMsg::Spectator { read_only, .. } => {
                read_only.as_deref()
            }
            RoleMsg::Pilot { .. } => None,
        }
    }

    /// The JSON text frame: `{"type":"role","role":…}`.
    pub fn to_frame(&self) -> String {
        let mut value = serde_json::to_value(self).expect("role message serialises");
        value["type"] = serde_json::Value::from("role");
        value.to_string()
    }
}

/// Normalise the `poste` query parameter. Anything that is not a short token of
/// `[A-Za-z0-9_-]` is ignored, and the socket then counts as a poste of its own.
pub fn sanitize_poste(raw: Option<&str>) -> Option<String> {
    let raw = raw?.trim();
    let ok = !raw.is_empty()
        && raw.len() <= 64
        && raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    ok.then(|| raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn size(cols: u16, rows: u16) -> TermSize {
        TermSize { cols, rows }
    }

    #[test]
    fn role_frames_are_tagged_json() {
        assert_eq!(RoleMsg::SOLO.to_frame(), r#"{"role":"solo","type":"role"}"#);
        let v: serde_json::Value = serde_json::from_str(
            &RoleMsg::Spectator {
                pilot: size(200, 50),
                read_only: None,
            }
            .to_frame(),
        )
        .unwrap();
        assert_eq!(v["type"], "role");
        assert_eq!(v["role"], "spectator");
        assert_eq!(v["pilot"]["cols"], 200);
        assert_eq!(v["pilot"]["rows"], 50);
        assert!(v.get("read_only").is_none(), "{v}");
    }

    #[test]
    fn a_read_only_frame_carries_the_refusal_sentence() {
        let why = "You can't attach this session here. Ask an administrator of the test extension.";
        let solo = RoleMsg::Solo {
            read_only: Some(why.into()),
        };
        assert_eq!(solo.read_only(), Some(why));
        let v: serde_json::Value = serde_json::from_str(&solo.to_frame()).unwrap();
        assert_eq!(v["role"], "solo");
        assert_eq!(v["read_only"], why);
        let spectator = RoleMsg::Spectator {
            pilot: size(80, 24),
            read_only: Some(why.into()),
        };
        let v: serde_json::Value = serde_json::from_str(&spectator.to_frame()).unwrap();
        assert_eq!(v["read_only"], why);
        assert_eq!(RoleMsg::Pilot { spectators: 1 }.read_only(), None);
    }

    #[test]
    fn sanitize_poste_accepts_tokens_only() {
        assert_eq!(sanitize_poste(Some("a1-B_2")), Some("a1-B_2".into()));
        assert_eq!(sanitize_poste(Some("")), None);
        assert_eq!(sanitize_poste(Some("a b")), None);
        assert_eq!(sanitize_poste(Some(&"x".repeat(65))), None);
        assert_eq!(sanitize_poste(None), None);
    }
}
