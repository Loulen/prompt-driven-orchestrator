/**
 * A remark plugin: a deep link written as plain text in markdown
 * (`#/ext/test/topbar/end/note/note-12`) becomes a link, so every markdown render
 * of the core makes it clickable (ADR-0081, amendment #29). Text inside a link,
 * inline code or a code block is left alone; trailing punctuation stays text.
 */

interface MdNode {
  type: string;
  value?: string;
  url?: string;
  children?: MdNode[];
}

const DEEP_LINK_TEXT = /#\/ext\/[a-z0-9][a-z0-9-]*(?:\/[^\s<>()[\]"'`]*)?/g;
const TRAILING_PUNCTUATION = /[.,;:!?]+$/;
/** Nodes whose text never turns into a link. */
const OPAQUE = new Set(["link", "linkReference", "inlineCode", "code", "html", "definition"]);

/** `value` split into text and links around each deep link it holds. */
export function splitDeepLinks(value: string): MdNode[] {
  const nodes: MdNode[] = [];
  let last = 0;
  for (const match of value.matchAll(DEEP_LINK_TEXT)) {
    const url = match[0].replace(TRAILING_PUNCTUATION, "");
    const start = match.index ?? 0;
    if (start > last) nodes.push({ type: "text", value: value.slice(last, start) });
    nodes.push({ type: "link", url, children: [{ type: "text", value: url }] });
    last = start + url.length;
  }
  if (nodes.length === 0) return [{ type: "text", value }];
  if (last < value.length) nodes.push({ type: "text", value: value.slice(last) });
  return nodes;
}

function linkify(node: MdNode): void {
  if (!node.children || OPAQUE.has(node.type)) return;
  node.children = node.children.flatMap((child) => {
    if (child.type === "text" && child.value?.includes("#/ext/")) return splitDeepLinks(child.value);
    linkify(child);
    return [child];
  });
}

export default function remarkExtensionLinks() {
  return (tree: MdNode) => linkify(tree);
}
