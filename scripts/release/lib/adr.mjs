// Draft ADRs at release time: `docs/adr/draft-<slug>.md`, cited
// `ADR draft-<slug>`, get the next numbers and every citation is rewritten.

const NUMBERED = /^(\d{4})-(.+)\.md$/;
const DRAFT = /^draft-(.+)\.md$/;

/**
 * The renames of the drafts among `names` (the files of `docs/adr/`), numbered
 * after the highest number, in name order.
 */
export function numberDrafts(names) {
  const numbered = names.map((n) => NUMBERED.exec(n)).filter(Boolean);
  const highest = Math.max(0, ...numbered.map((m) => Number(m[1])));
  const slugs = new Set(numbered.map((m) => m[2]));
  const drafts = names
    .map((n) => DRAFT.exec(n))
    .filter(Boolean)
    .map((m) => m[1])
    .sort();
  return drafts.map((slug, i) => {
    if (slugs.has(slug)) throw new Error(`draft-${slug}.md: an ADR with this slug is already numbered`);
    const number = String(highest + 1 + i).padStart(4, "0");
    return { slug, id: `ADR-${number}`, from: `draft-${slug}.md`, to: `${number}-${slug}.md` };
  });
}

const escape = (s) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/**
 * `text` with every citation of a renamed draft rewritten: its file name (in a
 * link or a path) becomes the numbered one, `ADR draft-<slug>`, ``ADR `draft-<slug>` ``
 * and a bare `draft-<slug>` become `ADR-NNNN`,
 * a citation wrapped after `ADR` included.
 */
export function rewriteCitations(text, renames) {
  let out = text;
  for (const { slug, id, from, to } of renames) {
    const name = `draft-${escape(slug)}`;
    // The slug must end there: `draft-a` is no citation of `draft-a-bis`.
    const end = "(?![\\w-])";
    // `ADR`, then spaces, a line break and the next line's comment marker if any.
    const adr = "ADR\\s+(?:(?://[/!]?|#|\\*)\\s*)?";
    out = out
      .replace(new RegExp(escape(from), "g"), to)
      .replace(new RegExp(`${adr}\`${name}\`${end}`, "g"), id)
      .replace(new RegExp(`${adr}${name}${end}`, "g"), id)
      .replace(new RegExp(`\`${name}\`${end}`, "g"), id)
      .replace(new RegExp(`(?<![\\w-])${name}${end}`, "g"), id);
  }
  return out;
}
