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
    <div className="border-b border-line" data-testid="diff-file-body">
      <div className="overflow-x-auto">
        <table
          className="w-max min-w-full border-collapse font-mono select-text"
          style={{ fontSize: "10.5px", lineHeight: "18px" }}
        >
          <tbody>
            {shown.map((r, idx) => {
              if (r.kind === "hunk") {
                return (
                  <tr key={idx} className="bg-surface-2/60 text-text-3" data-testid="diff-hunk">
                    <td
                      colSpan={2}
                      className="sticky left-0 z-[1] bg-surface-2 px-2 text-right select-none"
                      aria-hidden
                    />
                    <td className="whitespace-pre px-2 text-primary/70" colSpan={2}>
                      <span className="flex items-center gap-2">
                        <span>
                          {r.text}
                          {r.header && <span className="text-text-3"> {r.header}</span>}
                        </span>
                        {/* Context expansion lives on the Review page (#749). */}
                        <a
                          href={reviewHref}
                          className="ml-auto pl-4 text-text-3 hover:text-text-3"
                          title="Expand context on the Review page"
                          tabIndex={-1}
                        >
                          ↕ expand
                        </a>
                      </span>
                    </td>
                  </tr>
                );
              }
              const { line, segs } = r;
              const rowTone =
                line.kind === "add"
                  ? "bg-st-done/14"
                  : line.kind === "del"
                    ? "bg-st-failed/14"
                    : "";
              const gutterTone =
                line.kind === "add"
                  ? "bg-st-done/14"
                  : line.kind === "del"
                    ? "bg-st-failed/14"
                    : "bg-surface-1";
              const marker = line.kind === "add" ? "+" : line.kind === "del" ? "−" : " ";
              const markerTone =
                line.kind === "add"
                  ? "text-st-done"
                  : line.kind === "del"
                    ? "text-st-failed"
                    : "text-text-3";
              return (
                <tr key={idx} className={rowTone} data-testid={`diff-line-${line.kind}`}>
                  <td
                    className={`sticky left-0 z-[1] w-8 min-w-8 pl-2 pr-1 text-right text-text-3 select-none ${gutterTone}`}
                    aria-hidden
                  >
                    {line.old_no ?? ""}
                  </td>
                  <td
                    className={`sticky left-8 z-[1] w-8 min-w-8 pr-2 text-right text-text-3 select-none ${gutterTone}`}
                    aria-hidden
                  >
                    {line.new_no ?? ""}
                  </td>
                  <td className={`w-3 pl-2 select-none ${markerTone}`} aria-hidden>
                    {marker}
                  </td>
                  <td className={`whitespace-pre pr-4 ${line.kind === "context" ? "text-text-3" : "text-text"}`}>
                    {segs ? <Highlighted segs={segs} kind={line.kind} /> : line.content}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      {truncated && (
        <button
          onClick={onShowAll}
          className="w-full py-1.5 text-center text-primary transition-colors hover:bg-hover cursor-pointer"
          style={{ fontSize: "10.5px" }}
          data-testid="diff-show-more"
        >
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
  const tone = kind === "add" ? "bg-st-done/35" : "bg-st-failed/35";
  return (
    <>
      {segs.map((s, i) => (
        <Fragment key={i}>
          {s.changed ? (
            <span className={`rounded-[2px] ${tone}`} data-testid="diff-word">
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
