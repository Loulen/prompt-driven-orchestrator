//! The published support matrix — capability × harness, **rendered from the code
//! that declares it** (#617, ADR-0045/0051).
//!
//! PDO's instrumentation is unequal on purpose: everything beyond launching a
//! harness is a capability written harness by harness ([`crate::harness_probes`]),
//! and a harness that cannot do something says so rather than pretending. That
//! inequality is only honest if it is **published**. This module is where it gets
//! published.
//!
//! ## Why it is generated
//!
//! A hand-written table is wrong the first time a capability is added, and it is
//! exactly the kind of documentation nobody re-reads. So the table has **one source
//! of truth**: the ✅/❌ of every cell is read from
//! [`crate::harness_probes::probes_for`] at render time. Adding a harness, adding a
//! capability, or flipping one from present to absent moves the table with no
//! second edit — and `make check` fails if the committed block has drifted from
//! what this renders ([`check`]).
//!
//! ## What is declared here, and what is enforced
//!
//! Two things cannot be read off the dispatch table, so they are declared:
//!
//! - the **motive** of each absence ([`ABSENCES`]) — "why not" is prose, not a
//!   boolean. It is not left to discipline either:
//!   [`tests::every_absence_on_the_floor_has_a_motive`] fails if any embedded
//!   harness is absent on a capability with nothing to say about it. That is the
//!   ticket's promise made structural — "what is unsupported is documented", by
//!   construction rather than by care;
//! - the **last validated version** of each binary
//!   ([`crate::harness_registry::validated_version`]) — a documented bound, never a
//!   guard: PDO launches on any installed version, silently (out of scope, #612).
//!
//! The *mechanism* behind a present capability is not declared here either: each
//! capability enum labels its own variants (`CostSource::label`, …), so the table
//! can never name a mechanism the code no longer dispatches to.

use crate::harness_probes::probes_for;
use crate::harness_registry::{embedded_floor, validated_version, COPILOT, OPENCODE};

/// The HTML comment opening the generated block in the README. Everything between
/// this line and [`END_MARKER`] is owned by the generator.
pub const BEGIN_MARKER: &str = "<!-- support-table:begin -->";
/// The HTML comment closing the generated block.
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
            Capability::Cost => {
                "Turn a Run into a dollar figure. Absent ⇒ the Run's cost reads \
                                 \"—\" and names the harness, never `$0`"
            }
            Capability::Transcript => {
                "Find the session's transcript on disk — what cost and end-of-turn read"
            }
            Capability::TurnEnd => {
                "Complete a node by itself when its turn ends. Absent ⇒ the \
                                    agent runs `pdo complete`, or you do"
            }
            Capability::UsageLimit => {
                "Notice a session parked on the harness's usage-limit menu \
                                       (informational, no recovery)"
            }
            Capability::Staging => {
                "Hold a sandboxed session's staged home — credentials, \
                                    settings, pre-granted trust"
            }
            Capability::ContextUsage => {
                "Measure a session's context-window peak, in tokens, for Stats → \
                                        Performance (#585). Absent ⇒ no Context column for the \
                                        harness, never an invented reading"
            }
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

/// Why an embedded harness is **absent** on a capability. Declared, because a
/// motive is prose; enforced, because a missing entry fails a test rather than
/// publishing a bare ❌.
///
/// A data-declared harness needs no row: it is absent on all six for one reason —
/// PDO carries no code for it — and the block says that in a sentence.
const ABSENCES: &[(&str, Capability, &str)] = &[
    (
        OPENCODE,
        Capability::Cost,
        "It writes its own per-message cost into a SQLite in four buckets that do not map onto \
         `claude`'s. A cost is code, never a declared mini-language (ADR-0045), and nobody has \
         written that code yet.",
    ),
    (
        OPENCODE,
        Capability::Transcript,
        "It migrated its sessions into a SQLite and left months of dead JSON on disk. A store is \
         not a contract, so PDO declares no resolution rather than read zeros off stale files.",
    ),
    (
        OPENCODE,
        Capability::TurnEnd,
        "It exposes no end-of-turn signal PDO can read: its argv template carries no `{settings}` \
         hole for a `Stop` hook, and it has no transcript for a sweep to tail (see above).",
    ),
    (
        OPENCODE,
        Capability::UsageLimit,
        "The menu wording is `claude`'s. Matching another harness's pane against it would invent \
         a state, and the probe triggers no recovery anyway (ADR-0012).",
    ),
    (
        OPENCODE,
        Capability::Staging,
        "Configuring a harness is a documented prerequisite, not PDO code. A sandboxed Run on it \
         holds by your image and the profile's `$HOME` exceptions, and PDO says so once, visibly.",
    ),
    (
        OPENCODE,
        Capability::ContextUsage,
        "Its own SQLite reports token usage in four buckets that do not map onto `claude`'s \
         (see Cost above), and it carries no transcript PDO can tail (see Transcript above) — a \
         context peak is code, written per harness (#585), and nobody has written that code yet.",
    ),
    (
        COPILOT,
        Capability::UsageLimit,
        "The menu wording is `claude`'s, its own documentation admits the textual anchor drifts \
         each release, and the probe triggers no recovery (ADR-0012). Declaring it absent \
         degrades nothing actionable.",
    ),
    (
        COPILOT,
        Capability::Staging,
        "Configuring a harness is a documented prerequisite, not PDO code (ADR-0031). A sandboxed \
         Run on it holds by your image and the profile's `$HOME` exceptions, and PDO says so \
         once, visibly.",
    ),
];

/// The declared motive for `harness` being absent on `capability`, or `None` when
/// none was declared (which a test forbids for the embedded floor).
pub(crate) fn absence_motive(harness: &str, capability: Capability) -> Option<&'static str> {
    ABSENCES
        .iter()
        .find(|(h, c, _)| *h == harness && *c == capability)
        .map(|(_, _, why)| *why)
}

/// Render the support block — everything that goes between the two markers.
///
/// Pure: no IO, no clock. The harness axis is [`embedded_floor`] in declaration
/// order; the capability axis is [`Capability::ALL`]; every cell is read from the
/// dispatch table. Nothing here can be edited into a lie without the render moving
/// with it — which is what [`check`] then enforces on the committed file.
pub fn render() -> String {
    let harnesses: Vec<String> = embedded_floor().into_iter().map(|d| d.name).collect();

    let mut out = String::new();
    out.push_str(BEGIN_MARKER);
    out.push_str(
        "\n<!-- Generated from crates/pdo-daemon/src/harness_probes.rs. Do not edit by hand: \
         run `make support-table`. `make check` fails if this block has drifted. -->\n\n",
    );
    out.push_str(
        "PDO ships these harnesses compiled in. **Launching, attaching, resuming and completing a \
         node work on every one of them.** Everything *beyond* launching is a **capability**, \
         written harness by harness — and a harness that lacks one says so rather than quietly \
         doing nothing.\n\n",
    );

    // --- the matrix ---------------------------------------------------------
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
                Some(mechanism) => out.push_str(&format!(" ✅ {mechanism} |")),
                None => out.push_str(" ❌ |"),
            }
        }
        out.push('\n');
    }

    out.push_str(
        "\nThe version beside each harness is the **last validated** one — the build PDO's \
         knowledge of that harness was measured against. It is a documented bound, not a guard: \
         PDO launches on whatever version you have installed and says nothing about the \
         difference. It is written down because the same harness can sit on one machine twice, \
         months apart, with different event schemas and different model lists — and an inventory \
         taken against the wrong install is worse than no inventory.\n",
    );

    // --- the absences and their motives -------------------------------------
    let mut rows: Vec<(String, Capability)> = Vec::new();
    for h in &harnesses {
        for cap in Capability::ALL {
            if cap.mechanism(h).is_none() {
                rows.push((h.clone(), cap));
            }
        }
    }
    if !rows.is_empty() {
        out.push_str("\nWhy a capability is absent:\n\n");
        out.push_str("| Harness | Capability | Why |\n| --- | --- | --- |\n");
        for (h, cap) in rows {
            let why = absence_motive(&h, cap)
                .unwrap_or("*(undocumented — this is a bug: see `harness_support::ABSENCES`)*");
            out.push_str(&format!("| `{h}` | {} | {why} |\n", cap.title()));
        }
    }

    out.push_str(
        "\nA harness **you** declare in `~/.pdo/harnesses/descriptors.yaml` carries no code, so it \
         is absent on all six — it still launches, attaches, resumes, and completes when its \
         agent runs `pdo complete`. That is a legitimate way to run a harness, not a broken one.\n\n",
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
/// `Ok(())` on agreement. `Err(message)` **names the drift** — which is the whole
/// point: a diff that only says "differs" sends the reader back to guessing.
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
    use crate::harness_registry::CLAUDE;

    #[test]
    fn every_absence_on_the_floor_has_a_motive() {
        // The ticket's promise, made structural: "what is unsupported is
        // documented". Declare a harness, or flip a capability off, and this test
        // fails until the motive is written — the table can never publish a bare ❌.
        for d in embedded_floor() {
            for cap in Capability::ALL {
                if cap.mechanism(&d.name).is_none() {
                    let why = absence_motive(&d.name, cap).unwrap_or_else(|| {
                        panic!(
                            "{} is absent on {} with no declared motive — add one to \
                             harness_support::ABSENCES",
                            d.name,
                            cap.title()
                        )
                    });
                    assert!(
                        why.trim().len() > 20,
                        "{} / {}: a motive must actually say something",
                        d.name,
                        cap.title()
                    );
                }
            }
        }
    }

    #[test]
    fn no_motive_is_declared_for_a_capability_that_is_present() {
        // The other half: a stale motive for a capability that has since been
        // implemented would publish a contradiction. Nothing renders it, so this
        // catches it at the declaration.
        for (harness, cap, _) in ABSENCES {
            assert!(
                cap.mechanism(harness).is_none(),
                "{harness} implements {} — remove its stale absence motive",
                cap.title()
            );
        }
    }

    #[test]
    fn the_matrix_reads_the_dispatch_table_not_a_hand_written_list() {
        // claude: six present. copilot: four present, two absent. opencode: none.
        // These are read through `mechanism`, so this test pins the *wiring*, not a
        // duplicate of the capability declaration.
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
        // A harness PDO carries no code for: absent on all six, no per-name code.
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
        // A present cell names its mechanism; an absent one is a bare ❌ whose
        // motive lives in the second table.
        assert!(block.contains("✅ derived — per-message token usage × the price table"));
        assert!(block.contains("❌"));
        // Every absence motive is published.
        for (h, cap, why) in ABSENCES {
            assert!(
                block.contains(why),
                "{h} / {} motive not published",
                cap.title()
            );
        }
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
        // FP step 2: edit the table by hand so it lies → the check fails, naming
        // the drift rather than only reporting a difference.
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
        // FP step 3: regenerating puts the table back to what the code declares,
        // without touching a byte of prose around it.
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
        // Never rewrite a document PDO cannot locate the block in.
        let err = splice("# no markers here\n", &render()).expect_err("must refuse");
        assert!(err.contains("missing"), "{err}");
        let err = check("# no markers here\n").expect_err("must refuse");
        assert!(err.contains("missing"), "{err}");
        // Opened but never closed.
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
