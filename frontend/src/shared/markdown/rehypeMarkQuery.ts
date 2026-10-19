import type { Element, ElementContent, Root, Text } from "hast";

function escapeRegExp(text: string): string {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/** `text` cut into the parts that match `query` (ignoring case) and the rest. */
export function highlightParts(text: string, query: string): { text: string; match: boolean }[] {
  const needle = query.trim();
  if (!needle) return [{ text, match: false }];
  const parts: { text: string; match: boolean }[] = [];
  const re = new RegExp(escapeRegExp(needle), "giu");
  let from = 0;
  for (const m of text.matchAll(re)) {
    const at = m.index ?? 0;
    if (at > from) parts.push({ text: text.slice(from, at), match: false });
    parts.push({ text: m[0], match: true });
    from = at + m[0].length;
  }
  if (from < text.length || parts.length === 0) parts.push({ text: text.slice(from), match: false });
  return parts;
}

/** The class of a `<mark>` the plugin adds. */
export const QUERY_MARK_CLASS = "query-mark";

function mark(node: Root | Element, query: string): void {
  node.children = node.children.flatMap((child): ElementContent[] => {
    if (child.type === "text") {
      return highlightParts(child.value, query).map(
        (part): ElementContent =>
          part.match
            ? {
                type: "element",
                tagName: "mark",
                properties: { className: [QUERY_MARK_CLASS] },
                children: [{ type: "text", value: part.text } satisfies Text],
              }
            : ({ type: "text", value: part.text } satisfies Text),
      );
    }
    if (child.type === "element") mark(child, query);
    return [child as ElementContent];
  }) as Root["children"] & Element["children"];
}

/** A rehype plugin that wraps every match of `query` in the rendered
 *  Markdown in a `<mark>`: the search's word, seen in the text. */
export function rehypeMarkQuery({ query }: { query: string }) {
  return (tree: Root) => {
    if (query.trim()) mark(tree, query);
  };
}
