//! The published support matrix — capability × harness, **rendered from the code
//! that declares it** (#617, ADR-0045/0051).
//!
//! Everything beyond launching a harness is a capability written harness by
//! harness ([`crate::harness_probes`]). This module publishes that matrix.
//!
//! The table has **one source of truth**: every ✅/❌ is read from
//! [`crate::harness_probes::probes_for`] at render time, so adding a harness or a
//! capability moves the table with no second edit, and `make check` fails if the
//! committed block has drifted ([`check`]). Don't hand-write a cell.
//!
//! The **last validated version** of each binary cannot be read off the dispatch
//! table, so [`crate::harness_registry::validated_version`] declares it. This is a
//! documented bound, not a guard: PDO launches on any installed version (out of
//! scope, #612).
//!
//! The *mechanism* behind a present capability is not declared here either: each
//! capability enum labels its own variants (`CostSource::label`, …), so the table
//! can never name a mechanism the code no longer dispatches to.

use crate::harness_probes::probes_for;
use crate::harness_registry::{embedded_floor, validated_version};

/// The HTML comment opening the generated block in the README. Everything between
/// this line and [`END_MARKER`] is owned by the generator.
pub const BEGIN_MARKER: &str = "<!-- support-table:begin -->";
pub const END_MARKER: &str = "<!-- support-table:end -->";

/// One of the six capabilities the support table publishes, in publication order.
///
/// A closed enum is right *here* (unlike the harness axis, ADR-0045): the six are
/// the trait's six methods, so a seventh capability is a code change in
/// [`crate::harness_probes`] anyway — and this `match` is then the compiler's
/// reminder to publish it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Capability {
    Cost,
    Transcript,
    TurnEnd,
    UsageLimit,
    Staging,
    ContextUsage,
}

impl Capability {
    /// The six, in the order the table lists them.
    pub(crate) const ALL: [Capability; 6] = [
        Capability::Cost,
        Capability::Transcript,
        Capability::TurnEnd,
        Capability::UsageLimit,
        Capability::Staging,
        Capability::ContextUsage,
    ];

    /// The capability's column name in the published table.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Capability::Cost => "Cost",
            Capability::Transcript => "Transcript",
            Capability::TurnEnd => "End of turn",
            Capability::UsageLimit => "Usage-limit menu",
            Capability::Staging => "Sandbox staging floor",
            Capability::ContextUsage => "Context usage",
        }
    }

    /// What PDO does with the capability — one line, so a reader knows what an ❌
    /// actually costs them.
    pub(crate) fn blurb(self) -> &'static str {
        match self {
            Capability::Cost => "Show the Run cost",
            Capability::Transcript => "Find the session transcript",
            Capability::TurnEnd => "Complete a node when its turn ends",
            Capability::UsageLimit => "Detect the harness usage-limit menu",
            Capability::Staging => "Stage credentials, settings, and trust in a sandbox",
            Capability::ContextUsage => "Show peak context-window usage",
        }
    }

    /// The mechanism `harness` uses for this capability, or `None` when it is
    /// absent. Read from the **dispatch table**, so a cell can never claim a
    /// capability the code does not implement.
    pub(crate) fn mechanism(self, harness: &str) -> Option<&'static str> {
        let p = probes_for(harness)?;
        match self {
            Capability::Cost => p.cost_source().map(|c| c.label()),
            Capability::Transcript => p.transcript_resolution().map(|t| t.label()),
            Capability::TurnEnd => p.turn_end_substrate().map(|t| t.label()),
            Capability::UsageLimit => p.usage_limit_anchor().map(|u| u.label()),
            Capability::Staging => p.staging_floor().map(|s| s.label()),
            Capability::ContextUsage => p.context_usage_source().map(|c| c.label()),
        }
    }
}

/// Render the support block — everything that goes between the two markers.
///
/// Pure: no IO, no clock. Harness axis = [`embedded_floor`] in declaration order,
/// capability axis = [`Capability::ALL`], every cell read from the dispatch table.
pub fn render() -> String {
    let harnesses: Vec<String> = embedded_floor().into_iter().map(|d| d.name).collect();

    let mut out = String::new();
    out.push_str(BEGIN_MARKER);
    out.push_str(
        "\n<!-- Generated from crates/pdo-daemon/src/harness_probes.rs. Do not edit by hand: \
         run `make support-table`. `make check` fails if this block has drifted. -->\n\n",
    );
    out.push_str(
        "PDO can launch, attach, resume, and complete nodes with every built-in harness.\n\n",
    );

    out.push_str("| Capability | What PDO does with it |");
    for h in &harnesses {
        out.push_str(&format!(" `{h}` {} |", validated_version(h).unwrap_or("—")));
    }
    out.push_str("\n| --- | --- |");
    for _ in &harnesses {
        out.push_str(" --- |");
    }
    out.push('\n');

    for cap in Capability::ALL {
        out.push_str(&format!("| **{}** | {} |", cap.title(), cap.blurb()));
        for h in &harnesses {
            match cap.mechanism(h) {
                Some(mechanism) => {
                    let concise = mechanism.replace(" — ", ": ");
                    out.push_str(&format!(" ✅ {concise} |"));
                }
                None => out.push_str(" ❌ |"),
            }
        }
        out.push('\n');
    }

    out.push_str(
        "\nEach header shows the last validated harness version; PDO does not enforce it.\n",
    );

    out.push_str(
        "\nCustom descriptors in `~/.pdo/harnesses/descriptors.yaml` can launch, attach, resume, and \
         complete nodes through `pdo complete`.\n\n",
    );
    out.push_str(END_MARKER);
    out.push('\n');
    out
}

/// Splice a freshly rendered block into `document`, replacing whatever currently
/// sits between the markers. PURE — the caller does the IO.
///
/// `Err` when the markers are missing or inverted: a document PDO cannot locate the
/// block in is never rewritten wholesale.
pub fn splice(document: &str, block: &str) -> Result<String, String> {
    let (start, end) = locate(document)?;
    let mut out = String::with_capacity(document.len() + block.len());
    out.push_str(&document[..start]);
    out.push_str(block.trim_end());
    out.push_str(&document[end..]);
    Ok(out)
}

/// Whether the block committed in `document` still matches what [`render`] emits.
///
/// `Err(message)` **names the drift**: a diff that only says "differs" sends the
/// reader back to guessing.
pub fn check(document: &str) -> Result<(), String> {
    let block = render();
    let (start, end) = locate(document)?;
    let committed = document[start..end].trim_end();
    let expected = block.trim_end();
    if committed == expected {
        return Ok(());
    }
    Err(format!(
        "the harness support table has drifted from the code that declares it.\n\n{}\n\n\
         Regenerate it with `make support-table` (the table is generated from \
         crates/pdo-daemon/src/harness_probes.rs — edit the capabilities, not the README).",
        first_difference(committed, expected)
    ))
}

/// Byte offsets of the generated block inside `document`, markers included.
fn locate(document: &str) -> Result<(usize, usize), String> {
    let start = document.find(BEGIN_MARKER).ok_or_else(|| {
        format!("the generated block is missing: no `{BEGIN_MARKER}` marker in the document")
    })?;
    let end_marker = document[start..].find(END_MARKER).ok_or_else(|| {
        format!("the generated block is unterminated: no `{END_MARKER}` after `{BEGIN_MARKER}`")
    })?;
    Ok((start, start + end_marker + END_MARKER.len()))
}

/// The first line where the committed block and the rendered one disagree, shown
/// side by side. Line-level rather than a full diff: the block is short, and the
/// first divergence is always the one that explains the rest.
fn first_difference(committed: &str, expected: &str) -> String {
    let mut c = committed.lines();
    let mut e = expected.lines();
    let mut line = 0usize;
    loop {
        line += 1;
        let (a, b) = (c.next(), e.next());
        if a == b {
            if a.is_none() {
                return "(the blocks differ only in trailing whitespace)".to_string();
            }
            continue;
        }
        let end = "<end of block>";
        return format!(
            "first difference at line {line} of the block:\n  committed: {}\n  generated: {}",
            a.unwrap_or(end),
            b.unwrap_or(end),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness_registry::{CLAUDE, COPILOT, OPENCODE};

    #[test]
    fn the_matrix_reads_the_dispatch_table_not_a_hand_written_list() {
        // Read through `mechanism`, so this pins the *wiring* rather than
        // duplicating the capability declaration.
        assert!(Capability::ALL
            .iter()
            .all(|c| c.mechanism(CLAUDE).is_some()));
        assert!(Capability::Cost.mechanism(COPILOT).is_some());
        assert!(Capability::TurnEnd.mechanism(COPILOT).is_some());
        assert!(Capability::UsageLimit.mechanism(COPILOT).is_none());
        assert!(Capability::Staging.mechanism(COPILOT).is_none());
        assert!(Capability::ContextUsage.mechanism(COPILOT).is_some());
        assert!(Capability::ALL
            .iter()
            .all(|c| c.mechanism(OPENCODE).is_none()));
        assert!(Capability::ALL
            .iter()
            .all(|c| c.mechanism("my-custom-harness").is_none()));
    }

    #[test]
    fn render_names_every_harness_its_version_and_the_six_capabilities() {
        let block = render();
        for d in embedded_floor() {
            assert!(
                block.contains(&format!("`{}`", d.name)),
                "{} missing",
                d.name
            );
            let v = validated_version(&d.name).unwrap();
            assert!(block.contains(v), "{} version {v} missing", d.name);
        }
        for cap in Capability::ALL {
            assert!(block.contains(cap.title()), "{} missing", cap.title());
        }
        assert!(block.contains("✅ derived: per-message token usage × the price table"));
        assert!(block.contains("❌"));
        assert!(!block.contains("Why a capability is absent"));
        assert!(block.starts_with(BEGIN_MARKER));
        assert!(block.trim_end().ends_with(END_MARKER));
    }

    #[test]
    fn check_passes_on_a_document_carrying_the_rendered_block() {
        let doc = format!(
            "# Title\n\n## Support\n\n{}\n\n## After\n",
            render().trim_end()
        );
        assert_eq!(check(&doc), Ok(()));
    }

    #[test]
    fn check_names_the_drift_when_the_committed_table_lies() {
        let doc = format!("# Title\n\n{}\n", render().trim_end());
        let lying = doc.replace("❌ |", "✅ everything |");
        assert_ne!(lying, doc, "the fixture must actually differ");
        let err = check(&lying).expect_err("a lying table must fail the check");
        assert!(err.contains("drifted"), "{err}");
        assert!(err.contains("first difference at line"), "{err}");
        assert!(err.contains("make support-table"), "{err}");
    }

    #[test]
    fn splice_replaces_the_block_and_leaves_the_rest_untouched() {
        let doc = format!(
            "# Title\n\n## Support\n\n{}\n\n## Prerequisites\n\nkeep me\n",
            "<!-- support-table:begin -->\nstale nonsense\n<!-- support-table:end -->"
        );
        let fixed = splice(&doc, &render()).expect("markers are present");
        assert!(fixed.starts_with("# Title\n\n## Support\n\n"));
        assert!(fixed.ends_with("\n\n## Prerequisites\n\nkeep me\n"));
        assert!(!fixed.contains("stale nonsense"));
        assert_eq!(check(&fixed), Ok(()));
    }

    #[test]
    fn splice_and_check_refuse_a_document_with_no_markers() {
        let err = splice("# no markers here\n", &render()).expect_err("must refuse");
        assert!(err.contains("missing"), "{err}");
        let err = check("# no markers here\n").expect_err("must refuse");
        assert!(err.contains("missing"), "{err}");
        let err = check("<!-- support-table:begin -->\nrows\n").expect_err("must refuse");
        assert!(err.contains("unterminated"), "{err}");
    }

    #[test]
    fn splice_is_idempotent() {
        let doc = format!("# Title\n\n{}\n", render().trim_end());
        let once = splice(&doc, &render()).unwrap();
        let twice = splice(&once, &render()).unwrap();
        assert_eq!(once, twice);
    }
}
