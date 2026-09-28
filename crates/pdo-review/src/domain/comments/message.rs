//! The **one message** a batch of freshly sent comments produces for the
//! Pipeline Manager (CONTEXT.md « Envoi au manager »).

use super::ReviewComment;

/// The **one message** a batch of freshly sent comments produces for the
/// Pipeline Manager (CONTEXT.md « Envoi au manager »): per comment its id, the
/// anchor, the ref pair, the hunk excerpt, the text, and the instruction to
/// answer through `pdo review reply`. Never one message per comment — the
/// manager would answer N times.
pub fn batch_message(
    run_id: &str,
    from_label: &str,
    to_label: &str,
    comments: &[ReviewComment],
) -> String {
    let n = comments.len();
    let mut out = format!(
        "Review comments — {n} new on Run {run_id} ({from_label} → {to_label}).\n\
         A human reviewed the diff between these two Run refs and left the remarks below. \
         Each one is an immutable Run event with its own id.\n\n"
    );
    for c in comments {
        let short =
            |s: &Option<String>| s.as_deref().map(|x| x.chars().take(7).collect::<String>());
        let shas = match (short(&c.from_sha), short(&c.to_sha)) {
            (Some(f), Some(t)) => format!(" ({f} → {t})"),
            _ => String::new(),
        };
        out.push_str(&format!(
            "[{id}] {path}:{letter}{line} · {side} side · {from} → {to}{shas}\n",
            id = c.id,
            path = c.path,
            letter = c.side.letter(),
            line = c.line,
            side = c.side.as_str(),
            from = c.from_ref,
            to = c.to_ref,
        ));
        if !c.excerpt.is_empty() {
            for l in c.excerpt.lines() {
                out.push_str("    ");
                out.push_str(l);
                out.push('\n');
            }
        }
        out.push_str("  Comment:\n");
        for l in c.text.lines() {
            out.push_str("  ");
            out.push_str(l);
            out.push('\n');
        }
        out.push('\n');
    }
    out.push_str(
        "Reply to each comment with `pdo review reply <id> --text \"<your answer>\"` \
         (add `--resolved` once you consider it addressed; `pdo review list` lists the open ones). \
         Address the code the comment points at — fix it on the Run branch, or explain why not. \
         Comments themselves are immutable: never try to edit or delete one.",
    );
    out
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::sent;
    use super::*;

    #[test]
    fn batch_message_is_one_message_with_ids_anchors_refs_excerpts_and_reply_instruction() {
        let msg = batch_message(
            "run-1",
            "Fork point",
            "Run tip",
            &[sent("rc-001", 3), sent("rc-002", 5)],
        );
        assert!(msg.starts_with("Review comments — 2 new on Run run-1 (Fork point → Run tip)."));
        assert!(msg.contains("[rc-001] frontend/src/pages/ReviewPage.tsx:R3 · new side · fork → tip (a1b2c3d → d4e5f6a)"));
        assert!(msg.contains("[rc-002] frontend/src/pages/ReviewPage.tsx:R5"));
        assert!(msg.contains("    > 3 | l3"), "excerpt with marker: {msg}");
        assert!(msg.contains("  `canSend` should be false\n  while sending."));
        assert!(msg.contains("pdo review reply <id> --text"));
        assert!(msg.contains("pdo review list"));
        assert!(msg.contains("immutable"));
    }
}
