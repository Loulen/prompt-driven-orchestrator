import type { StatusTone } from "../../shared/ui/kit/tones";

/**
 * The YAML frontmatter fence (`---` … `---`) an artifact may open with: the
 * viewer shows it as a card and renders the markdown after it.
 */

/** The leading fence of `content`: the YAML inside it and the text after it. */
function splitFence(content: string): { yaml: string; rest: string } | null {
  const trimmed = content.trimStart();
  if (!trimmed.startsWith("---")) return null;
  const after = trimmed.slice(3);
  const end = after.indexOf("\n---");
  if (end === -1) return null;
  return { yaml: after.slice(0, end), rest: after.slice(end + 4) };
}

/** `content` without its leading frontmatter block, or unchanged without one. */
export function stripFrontmatter(content: string): string {
  const fence = splitFence(content);
  return fence ? fence.rest.trimStart() : content;
}

/**
 * The fields of an artifact's frontmatter in the order its fence writes them. The
 * daemon answers a map, whose key order is not the file's: the raw `content` gives
 * it back. A field the fence does not show (no content yet) follows the others.
 */
export function frontmatterEntries(
  fields: Record<string, unknown>,
  content: string | null,
): [string, unknown][] {
  const order = fenceKeys(content);
  const rank = (key: string) => {
    const i = order.indexOf(key);
    return i === -1 ? order.length : i;
  };
  return Object.entries(fields)
    .map((entry, i) => ({ entry, i }))
    .sort((a, b) => rank(a.entry[0]) - rank(b.entry[0]) || a.i - b.i)
    .map(({ entry }) => entry);
}

/** The top-level keys of the leading fence, in order. */
function fenceKeys(content: string | null): string[] {
  const fence = content ? splitFence(content) : null;
  if (!fence) return [];
  return fence.yaml
    .split("\n")
    .map((line) => /^([^\s#][^:]*):(\s|$)/.exec(line)?.[1].trim().replace(/^["']|["']$/g, ""))
    .filter((key): key is string => !!key);
}

/** A frontmatter value as the strip reads it: a list as its items, a map as its pairs. */
export function frontmatterValue(value: unknown): string {
  if (value == null) return "";
  if (Array.isArray(value)) return value.map(nested).join(", ");
  if (typeof value === "object") {
    return Object.entries(value as Record<string, unknown>)
      .map(([k, v]) => `${k}: ${nested(v)}`)
      .join(", ");
  }
  return String(value);
}

function nested(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(nested).join(", ")}]`;
  if (value != null && typeof value === "object") return `{${frontmatterValue(value)}}`;
  return frontmatterValue(value);
}

/** The verdicts a frontmatter value may carry, and their tone. */
const VERDICT_TONE: Record<string, StatusTone> = {
  pass: "done",
  passed: "done",
  approved: "done",
  ok: "done",
  done: "done",
  finished: "done",
  true: "done",
  fail: "failed",
  failed: "failed",
  rejected: "failed",
  false: "failed",
  needs_work: "awaiting",
  pending: "awaiting",
};

/** The tone a frontmatter value is painted in when it reads as a verdict. */
export function verdictTone(value: unknown): StatusTone | undefined {
  return VERDICT_TONE[frontmatterValue(value).toLowerCase()];
}

/** Whether a frontmatter value is a web address the viewer can open. */
export function isWebUrl(text: string): boolean {
  return /^https?:\/\/\S+$/i.test(text.trim());
}

/**
 * The fields with the verdict first: a field named `verdict`, else the first
 * value that reads as one. The others keep their order.
 */
export function verdictFirst(
  values: Record<string, unknown> | ReadonlyArray<readonly [string, unknown]>,
): [string, unknown][] {
  const entries: [string, unknown][] = (Array.isArray(values) ? values : Object.entries(values)).map(
    ([k, v]) => [k, v],
  );
  let at = entries.findIndex(([k]) => k.toLowerCase() === "verdict");
  if (at === -1) at = entries.findIndex(([, v]) => verdictTone(v) !== undefined);
  if (at <= 0) return entries;
  return [entries[at], ...entries.slice(0, at), ...entries.slice(at + 1)];
}

/** Longer than this, a value takes a line of its own in the I/O rows. */
const WIDE_VALUE = 32;

/** Whether a value needs a line of its own: a web address or a long text. */
export function isWideValue(text: string): boolean {
  return isWebUrl(text) || text.length > WIDE_VALUE;
}
