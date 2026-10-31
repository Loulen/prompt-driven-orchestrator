// The CHANGELOG at release time: the `## Unreleased` entry the tickets wrote
// becomes the version's entry.

const HEADING = /^## (.*)$/;
const UNRELEASED = /^Unreleased( · .*)?$/;

/**
 * `text` with its `## Unreleased[ · title]` heading renamed `## <version>[ · title]`.
 * Refused when the entry is missing, is not the first one, or says nothing.
 */
export function releaseChangelog(text, version) {
  const lines = text.split("\n");
  const headings = lines.flatMap((line, i) => (HEADING.test(line) ? [i] : []));
  const at = headings.find((i) => UNRELEASED.test(HEADING.exec(lines[i])[1]));
  if (at === undefined) throw new Error("the CHANGELOG has no `## Unreleased` entry to release");
  if (at !== headings[0]) throw new Error("`## Unreleased` must be the first entry of the CHANGELOG");
  const end = headings[1] ?? lines.length;
  if (lines.slice(at + 1, end).every((line) => line.trim() === "")) {
    throw new Error("Unreleased is empty: write what the release brings first");
  }
  const title = UNRELEASED.exec(HEADING.exec(lines[at])[1])[1] ?? "";
  lines[at] = `## ${version}${title}`;
  return lines.join("\n");
}
