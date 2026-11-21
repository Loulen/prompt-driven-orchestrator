/**
 * A Rapport de bug sent to the core's public issues: the URL of a new issue,
 * on the « Bug report » template (`.github/ISSUE_TEMPLATE/bug_report.yml`),
 * each field of the template prefilled from the report's section of the same
 * id. The prompts never go: the template has no field for them. A URL too
 * long for GitHub drops node rows from the details, Run first, and says how
 * many it dropped.
 */

import type { BugReportFields, SectionKey } from "./bugReport";
import { truncatedLine } from "./bugReportDetails";

export const CORE_ISSUES_NEW_URL = "https://github.com/Loulen/prompt-driven-orchestrator/issues/new";
export const BUG_REPORT_TEMPLATE = "bug_report.yml";

/** Past this length GitHub refuses the URL (414); keep a margin. */
export const MAX_URL_LENGTH = 8000;

/** The template's fields, by their `id`: the report's sections bar the prompts. */
export const TEMPLATE_FIELDS: readonly SectionKey[] = ["description", "version", "os", "harnesses", "run", "pipeline"];

/** The issue's title: the description's first line, shortened. */
export function issueTitle(description: string | undefined): string {
  const first = (description ?? "").split("\n").find((l) => l.trim() !== "")?.trim() ?? "";
  return first.length > 80 ? `${first.slice(0, 79)}…` : first;
}

function build(fields: BugReportFields): string {
  const params: [string, string][] = [["template", BUG_REPORT_TEMPLATE]];
  const title = issueTitle(fields.description);
  if (title) params.push(["title", title]);
  for (const key of TEMPLATE_FIELDS) {
    const value = fields[key];
    if (value) params.push([key, value]);
  }
  const query = params.map(([k, v]) => `${k}=${encodeURIComponent(v)}`).join("&");
  return `${CORE_ISSUES_NEW_URL}?${query}`;
}

/**
 * `details` without its last `omit` table rows, and a line that says so. The
 * rows are the lines after a table's `| --- |` separator; text without a
 * table is left as it is.
 */
export function dropRows(details: string, omit: number): string {
  const lines = details.split("\n");
  const sep = lines.findIndex((l) => /^\|(\s*-+\s*\|)+\s*$/.test(l));
  if (sep < 0 || omit <= 0) return details;
  let end = sep + 1;
  while (end < lines.length && lines[end].startsWith("|")) end++;
  const rows = end - sep - 1;
  const n = Math.min(omit, rows);
  if (n === 0) return details;
  const kept = [...lines.slice(0, end - n), truncatedLine(n), ...lines.slice(end)];
  return kept.join("\n");
}

function rowCount(details: string | undefined): number {
  if (!details) return 0;
  const lines = details.split("\n");
  const sep = lines.findIndex((l) => /^\|(\s*-+\s*\|)+\s*$/.test(l));
  if (sep < 0) return 0;
  let n = 0;
  while (sep + 1 + n < lines.length && lines[sep + 1 + n].startsWith("|")) n++;
  return n;
}

/**
 * The URL of the new issue. When it is longer than `max`, node rows go, the
 * Run's then the Pipeline's; past that, the longest field is cut.
 */
export function githubIssueUrl(fields: BugReportFields, max: number = MAX_URL_LENGTH): string {
  let url = build(fields);
  if (url.length <= max) return url;
  const out: BugReportFields = { ...fields };
  for (const key of ["run", "pipeline"] as const) {
    const source = fields[key];
    const rows = rowCount(source);
    for (let omit = 1; omit <= rows && url.length > max; omit++) {
      out[key] = dropRows(source!, omit);
      url = build(out);
    }
  }
  while (url.length > max) {
    const longest = TEMPLATE_FIELDS.filter((k) => out[k]).sort((a, b) => out[b]!.length - out[a]!.length)[0];
    if (!longest) break;
    const text = out[longest]!;
    const over = url.length - max;
    // An encoded character takes up to 9 URL characters: cut generously.
    const keep = Math.max(0, text.length - Math.ceil(over / 3) - 20);
    out[longest] = keep > 0 ? `${text.slice(0, keep)}\n(truncated)` : "(truncated)";
    if (keep === 0 && out[longest] === text) break;
    url = build(out);
  }
  return url;
}
