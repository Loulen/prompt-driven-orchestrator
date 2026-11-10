//! The user-declared descriptor file: its grammar and what PDO refuses in it
//! (ADR-0045, ADR-0054). Pure: text in, descriptors and refusals out.

use std::collections::BTreeMap;

use serde::Deserialize;

use super::HarnessDescriptor;

/// A refused disk descriptor and why — the material of BOTH the `warn!` and the
/// `GET /settings` `reason`, so the two can never drift (idiom of
/// `price_table::RejectedRow`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedDescriptor {
    pub name: String,
    pub why: String,
}

/// The result of parsing the on-disk descriptor tier. Pure: text in; the parsed
/// descriptors and any refusals out. An unparseable document yields
/// `unparseable: Some(err)` and NO descriptors — never an `Err`, because a bad
/// descriptor file must not be able to fail a spawn (it degrades to the floor).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ParsedDescriptors {
    pub descriptors: Vec<HarnessDescriptor>,
    pub rejected: Vec<RejectedDescriptor>,
    pub unparseable: Option<String>,
}

/// One row of the descriptor file. The harness **name is the map key**, so it is
/// not a field here (key uniqueness is structural, like `price_table`'s manual
/// map). No `deny_unknown_fields` (ADR-0015 #471: an unknown field is ignored).
#[derive(Debug, Deserialize)]
struct DescriptorRow {
    binary: Option<String>,
    #[serde(default)]
    launch: Vec<String>,
    #[serde(default)]
    resume: Vec<String>,
    /// The resume-by-identity verb (`--resume`) that fills the resume template's
    /// `{resume}` hole (#614). Optional — a disk descriptor whose resume tail bakes
    /// its verb literally (the shape the pre-#614 examples used) leaves it empty.
    #[serde(default)]
    resume_by_id: String,
    /// The blind-resume verb (`--continue`) for a node with no recorded identity
    /// (#614). Optional, same default as [`Self::resume_by_id`].
    #[serde(default)]
    resume_blind: String,
    #[serde(default)]
    env: BTreeMap<String, String>,
}

/// Whether the launch template makes the **declared binary the leader of the
/// pane** — the one thing PDO validates about a descriptor (ADR-0054).
///
/// The check is syntactic (a proxy, ADR-0054 §"Limites"): the template must begin
/// with the token `exec` immediately followed by the declared `binary`. `exec`
/// replaces the pane's shell with the harness, so the harness — not a surviving
/// shell — is the pane leader, which is the precondition of PDO's only terminal
/// liveness verdict (ADR-0032): the harness dies ⇒ the session dies ⇒ the node
/// fails visibly. A template that wraps the binary in a shell, or omits `exec`,
/// leaves the shell leader: the harness can die while the session lives on, and
/// the node stays `Running` forever, mute. That descriptor is refused.
pub fn launch_makes_binary_leader(launch: &[String], binary: &str) -> bool {
    launch.first().map(String::as_str) == Some("exec")
        && launch.get(1).map(String::as_str) == Some(binary)
}

/// Parse the descriptor file (`descriptors.yaml`). PURE.
///
/// Shape — a MAP keyed by harness name, mirroring `price_table`'s `models.yaml`:
///
/// ```yaml
/// harnesses:
///   my-harness:
///     binary: my-harness
///     launch: ["exec", "my-harness", "--auto", "--prompt {prompt}"]
///     resume: ["exec", "my-harness", "--auto", "--continue"]   # optional
///     env: { FOO: bar }                                        # optional
/// ```
///
/// A row is **refused whole** (never partially applied) when it lacks the two
/// things PDO needs to *launch* it: a `binary` to probe and a non-empty `launch`
/// argv template (ADR-0045). A refused row's key falls through to the next tier —
/// so a broken `claude:` override leaves the embedded `claude` untouched. PDO does
/// **not** validate what the argv *means* (ADR-0001): a descriptor without an
/// autonomy flag simply yields a node stalled on a permission dialog, which is
/// said in the descriptor's documentation, not guarded here.
pub fn parse_descriptors(text: &str) -> ParsedDescriptors {
    #[derive(Deserialize)]
    struct Doc {
        #[serde(default)]
        harnesses: BTreeMap<String, DescriptorRow>,
    }

    // A blank/comment-only file is an empty tier, not a broken one — the normal
    // state of an instance that has the dir but no descriptor yet.
    if text.trim().is_empty() {
        return ParsedDescriptors::default();
    }
    let doc: Doc = match serde_yaml::from_str(text) {
        Ok(d) => d,
        Err(e) => {
            return ParsedDescriptors {
                unparseable: Some(e.to_string()),
                ..Default::default()
            }
        }
    };

    let mut parsed = ParsedDescriptors::default();
    for (name, row) in doc.harnesses {
        if name.trim().is_empty() {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: "a harness name must not be blank".to_string(),
            });
            continue;
        }
        let binary = row.binary.unwrap_or_default();
        if binary.trim().is_empty() {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: "missing `binary` — PDO probes a program at spawn; a descriptor with none \
                      could only ever fail-fast"
                    .to_string(),
            });
            continue;
        }
        if row.launch.is_empty() {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: "missing `launch` — a harness declares itself by an argv template (ADR-0045)"
                    .to_string(),
            });
            continue;
        }
        if !launch_makes_binary_leader(&row.launch, &binary) {
            parsed.rejected.push(RejectedDescriptor {
                name,
                why: format!(
                    "launch does not make the binary `{binary}` the pane leader — it must begin \
                     `exec {binary} …` (ADR-0054); otherwise the harness can die without the \
                     session dying and the node hangs `Running` forever"
                ),
            });
            continue;
        }
        parsed.descriptors.push(HarnessDescriptor {
            name,
            binary,
            launch: row.launch,
            resume: row.resume,
            resume_by_id: row.resume_by_id,
            resume_blind: row.resume_blind,
            env: row.env.into_iter().collect(),
        });
    }
    parsed
}
