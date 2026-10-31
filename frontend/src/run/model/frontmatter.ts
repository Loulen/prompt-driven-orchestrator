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
