import { useMemo, Fragment } from "react";
import { wordDiff } from "../../lib/wordDiff";
import { TRUNCATE_LINES } from "../model/diffTab";
import type { DiffFile } from "../../shared/generated/DiffFile";
import type { DiffLine } from "../../shared/generated/DiffLine";

// ---------------------------------------------------------------------------
// File body: unified hunks with a sticky old/new gutter, intra-line word
// highlight on adjacent `-`/`+` runs, truncation past TRUNCATE_LINES.
// ---------------------------------------------------------------------------

type Row =
  | { kind: "hunk"; header: string; text: string }
  | { kind: "line"; line: DiffLine; segs: { text: string; changed: boolean }[] | null };

function buildRows(file: DiffFile): Row[] {
  const rows: Row[] = [];
  for (const h of file.hunks) {
    rows.push({
      kind: "hunk",
      header: h.header,
      text: `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@`,
    });
    // Pair each run of `-` lines with the run of `+` lines that follows it,
    // index-wise (GitHub's rule), and word-diff each pair.
    const lines = h.lines;
    let i = 0;
    while (i < lines.length) {
      if (lines[i].kind !== "del") {
        rows.push({ kind: "line", line: lines[i], segs: null });
        i++;
        continue;
      }
      let j = i;
      while (j < lines.length && lines[j].kind === "del") j++;
      let k = j;
      while (k < lines.length && lines[k].kind === "add") k++;
      const dels = lines.slice(i, j);
      const adds = lines.slice(j, k);
      const pairs = Math.min(dels.length, adds.length);
      const delSegs: (Row & { kind: "line" })["segs"][] = dels.map(() => null);
      const addSegs: (Row & { kind: "line" })["segs"][] = adds.map(() => null);
      for (let p = 0; p < pairs; p++) {
        const d = wordDiff(dels[p].content, adds[p].content);
        // Highlight only when something is shared: an entirely different line
        // painted red/green end to end says nothing the row colour does not.
        const shared = d.old.some((s) => !s.changed) || d.new.some((s) => !s.changed);
        if (shared) {
          delSegs[p] = d.old;
          addSegs[p] = d.new;
        }
      }
      dels.forEach((l, p) => rows.push({ kind: "line", line: l, segs: delSegs[p] }));
      adds.forEach((l, p) => rows.push({ kind: "line", line: l, segs: addSegs[p] }));
      i = k;
    }
  }
  return rows;
}

export default function FileBody({
  file,
  reviewHref,
  full,
  onShowAll,
}: {
  file: DiffFile;
  /** The Review page, where context expansion lives (#749). */
  reviewHref: string;
  full: boolean;
  onShowAll: () => void;
}) {
  const rows = useMemo(() => buildRows(file), [file]);
  const lineRows = rows.filter((r) => r.kind === "line").length;
  const truncated = !full && lineRows > TRUNCATE_LINES;
  let shown = rows;
  if (truncated) {
    let count = 0;
    let cut = rows.length;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i].kind === "line") count++;
      if (count >= TRUNCATE_LINES) {
        cut = i + 1;
        break;
      }
    }
    shown = rows.slice(0, cut);
  }
  const hidden = lineRows - TRUNCATE_LINES;

  return (
    <div className="pi-hunks" data-testid="diff-file-body">
      <table>
        <tbody>
          {shown.map((r, idx) => {
            if (r.kind === "hunk") {
              return (
                <tr key={idx} className="pi-hunk__head" data-testid="diff-hunk">
                  <td colSpan={2} className="pi-hunk__gut pi-hunk__gut--old" aria-hidden />
                  <td className="pi-hunk__t" colSpan={2}>
                    <span className="ei-line">
                      <span>
                        {r.text}
                        {r.header && <span className="ei-dim"> {r.header}</span>}
                      </span>
                      {/* Context expansion lives on the Review page (#749). */}
                      <a href={reviewHref} className="pi-expand" title="Expand context on the Review page" tabIndex={-1}>
                        ↕ expand
                      </a>
                    </span>
                  </td>
                </tr>
              );
            }
            const { line, segs } = r;
            const marker = line.kind === "add" ? "+" : line.kind === "del" ? "−" : " ";
            return (
              <tr key={idx} className="pi-hunk__line" data-kind={line.kind} data-testid={`diff-line-${line.kind}`}>
                <td className="pi-hunk__gut pi-hunk__gut--old" aria-hidden>
                  {line.old_no ?? ""}
                </td>
                <td className="pi-hunk__gut pi-hunk__gut--new" aria-hidden>
                  {line.new_no ?? ""}
                </td>
                <td className="pi-hunk__sign" aria-hidden>
                  {marker}
                </td>
                <td className="pi-hunk__t">{segs ? <Highlighted segs={segs} kind={line.kind} /> : line.content}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
      {truncated && (
        <button type="button" onClick={onShowAll} className="pi-more" data-testid="diff-show-more">
          Show {hidden} more lines
        </button>
      )}
    </div>
  );
}

function Highlighted({
  segs,
  kind,
}: {
  segs: { text: string; changed: boolean }[];
  kind: DiffLine["kind"];
}) {
  return (
    <>
      {segs.map((s, i) => (
        <Fragment key={i}>
          {s.changed ? (
            <span className="pi-word" data-kind={kind} data-testid="diff-word">
              {s.text}
            </span>
          ) : (
            s.text
          )}
        </Fragment>
      ))}
    </>
  );
}
