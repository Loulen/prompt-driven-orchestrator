import type { DiffHunk } from "../../shared/generated/DiffHunk";
import { commentState } from "./commentStatus";
import type { CommentState } from "./commentStatus";
import type { ReviewEntry } from "./entries";

// Where a file card of the Review page puts its comments (#750, #751, #752):
// inline in the library's extend slots, in the outdated group, or above the
// body for a draft whose line is gone — and the per-card CSS that marks the
// anchored lines.

/** Line dot / bar colour per sent-comment state (#751). */
const STATE_VAR: Record<CommentState, string> = {
  open: "var(--color-st-running)",
  proposed: "var(--color-st-await)",
  resolved: "var(--color-st-done)",
};

/** #752: drafts anchored on a line the displayed hunks no longer show — listed above the body. */
export function orphanDraftsOf(entries: ReviewEntry[], hunks: DiffHunk[]): ReviewEntry[] {
  const drafts = entries.filter((e) => e.kind === "draft");
  if (drafts.length === 0) return [];
  const present = new Set<string>();
  for (const h of hunks) {
    for (const l of h.lines) {
      if (l.old_no != null) present.add(`old:${l.old_no}`);
      if (l.new_no != null) present.add(`new:${l.new_no}`);
    }
  }
  return drafts.filter((e) => !present.has(`${e.anchor.side}:${e.anchor.line}`));
}

/** The library's extend slots: one entry per anchored line, resolved ones dropped when hidden. */
export function extendDataOf(inlineEntries: ReviewEntry[], showResolved: boolean) {
  const oldFile: Record<string, { data: ReviewEntry }> = {};
  const newFile: Record<string, { data: ReviewEntry }> = {};
  for (const e of inlineEntries) {
    if (!showResolved && e.kind === "sent" && e.comment.status === "resolved") continue;
    const slot = e.anchor.side === "old" ? oldFile : newFile;
    // Sent wins over a stale draft on the same line (entries are sorted so).
    if (!slot[String(e.anchor.line)]) slot[String(e.anchor.line)] = { data: e };
  }
  return { oldFile, newFile };
}

/** Per-card CSS: dot + bar on anchored lines, `+` hidden where a comment sits. */
export function anchorCssOf(inlineEntries: ReviewEntry[], cssId: string): string {
  if (inlineEntries.length === 0) return "";
  const rules: string[] = [];
  for (const e of inlineEntries) {
    const color = e.kind === "draft" ? "var(--color-st-await)" : STATE_VAR[commentState(e.comment)];
    const { side, line } = e.anchor;
    // Split: `td.diff-line-<side>-num > span[data-line-num]`, content cell next.
    const num = `#${cssId} td.diff-line-${side}-num:has(> span[data-line-num="${line}"])`;
    rules.push(`${num}::before{content:"";position:absolute;left:5px;top:7px;width:6px;height:6px;border-radius:50%;background:${color}}`);
    rules.push(`${num} + td.diff-line-${side}-content{box-shadow:inset 2px 0 0 ${color}}`);
    rules.push(`${num} [data-add-widget]{display:none}`);
    // Unified: both numbers on one row, `span[data-line-<side>-num]`.
    const row = `#${cssId} tr.diff-line:has(span[data-line-${side}-num="${line}"])`;
    rules.push(`${row} td.diff-line-num{position:relative}`);
    rules.push(`${row} td.diff-line-num::before{content:"";position:absolute;left:5px;top:7px;width:6px;height:6px;border-radius:50%;background:${color}}`);
    rules.push(`${row} td.diff-line-content{box-shadow:inset 2px 0 0 ${color}}`);
    rules.push(`${row} [data-add-widget]{display:none}`);
  }
  return rules.join("\n");
}
