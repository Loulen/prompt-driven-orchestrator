/** The « New run » attachments zone (#779): the pure rules the modal applies. */

/** #779: the built-in per-run attachment budget, used until `GET /settings` answers. */
export const DEFAULT_MAX_ATTACHMENTS_MB = 50;

/** `18 KB`, `2.4 MB` — the sizes the attachment chips and counter show (#779). */
export function formatAttachmentSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** The uppercase extension badge of a non-image chip: `MD`, `JSON`, `PARQ` (≤ 4 chars). */
export function attachmentBadge(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "FILE";
  return name.slice(dot + 1).toUpperCase().slice(0, 4);
}

/** The result of merging a drop into the current attachments (#779). Pure, so the
 *  refusal rules are testable without a DOM: same name ⇒ replace the earlier one;
 *  a file that would push the total over the budget is refused, the rest of the
 *  same drop is still added. */
export function mergeAttachments(
  current: File[],
  incoming: File[],
  maxBytes: number,
): { next: File[]; replaced: string[]; refused: File[] } {
  let next = [...current];
  const replaced: string[] = [];
  const refused: File[] = [];
  for (const file of incoming) {
    const dup = next.findIndex((f) => f.name === file.name);
    const without = dup >= 0 ? next.filter((_, i) => i !== dup) : next;
    const total = without.reduce((sum, f) => sum + f.size, 0) + file.size;
    if (total > maxBytes) {
      refused.push(file);
      continue;
    }
    if (dup >= 0) replaced.push(file.name);
    next = dup >= 0 ? [...without.slice(0, dup), file, ...without.slice(dup)] : [...without, file];
  }
  return { next, replaced, refused };
}

/** Indices of the attachments sitting past the budget line (total already over,
 *  e.g. after the limit was lowered in Settings). Cumulative, in display order. */
export function overLimitIndices(attachments: File[], maxBytes: number): Set<number> {
  const over = new Set<number>();
  let running = 0;
  attachments.forEach((f, i) => {
    running += f.size;
    if (running > maxBytes) over.add(i);
  });
  return over;
}
