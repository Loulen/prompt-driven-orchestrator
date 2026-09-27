/**
 * The YAML frontmatter fence (`---` … `---`) an artifact may open with: the
 * viewer shows it as a card and renders the markdown after it.
 */

/** `content` without its leading frontmatter block, or unchanged without one. */
export function stripFrontmatter(content: string): string {
  const trimmed = content.trimStart();
  if (!trimmed.startsWith("---")) return content;
  const after = trimmed.slice(3);
  const end = after.indexOf("\n---");
  if (end === -1) return content;
  return after.slice(end + 4).trimStart();
}
