/**
 * The mounted pages as the app shows them (CONTEXT.md « Page montée »): the deep link
 * `#/pages/<name>` that opens a page in a tab of the center panel, the addresses
 * `pdo page mount` prints, and where a guide entry's link leads. Pure: no React, no fetch.
 */

/** The app's deep link toward a mounted page: its tab, with its guide. */
export const PAGE_LINK_PREFIX = "#/pages/";

/** The name of the page `href` (or a hash, or a URL ending with one) opens; `null` if none. */
export function parsePageLink(href: string): string | null {
  const at = href.indexOf(PAGE_LINK_PREFIX);
  if (at < 0) return null;
  const name = href.slice(at + PAGE_LINK_PREFIX.length).replace(/\/+$/, "");
  return /^[a-z0-9_-]{1,64}$/.test(name) ? name : null;
}

/** The deep link of the page mounted as `name`. */
export function formatPageLink(name: string): string {
  return `${PAGE_LINK_PREFIX}${name}`;
}

/** Where the page mounted as `name` is served alone, under `base` (`""` for this origin). */
export function pageUrl(base: string, name: string): string {
  return `${base.replace(/\/+$/, "")}/pages/${name}/`;
}

/** PDO's view of the page mounted as `name`, with its guide, under `base`. */
export function guidedPageUrl(base: string, name: string): string {
  return `${base.replace(/\/+$/, "")}/${formatPageLink(name)}`;
}

/** Where a guide entry's link leads (the daemon checked it at mount time). */
export type GuideTarget =
  | { kind: "run"; runId: string }
  | { kind: "node"; runId: string; nodeId: string; iter: number | null }
  | { kind: "artifact"; runId: string; nodeId: string; port: string }
  | { kind: "review"; runId: string; file: string | null; href: string }
  | { kind: "extension"; href: string }
  | { kind: "anchor"; anchor: string }
  | { kind: "path"; path: string };

/** Whether `target` leads somewhere in PDO (↗) rather than in the page (#). */
export function isPdoTarget(target: GuideTarget): boolean {
  return target.kind !== "anchor" && target.kind !== "path";
}

function decode(segment: string): string {
  try {
    return decodeURIComponent(segment);
  } catch {
    return segment;
  }
}

/** What a guide link points at. */
export function guideTarget(link: string): GuideTarget {
  if (link.startsWith("#/runs/")) {
    const [runId = "", nodes, nodeId = "", what, value = ""] = link
      .slice("#/runs/".length)
      .replace(/\/+$/, "")
      .split("/")
      .map(decode);
    if (nodes === "nodes" && what === "artifacts") {
      return { kind: "artifact", runId, nodeId, port: value };
    }
    if (nodes === "nodes") {
      const iter = what === "iter" && /^[1-9][0-9]*$/.test(value) ? Number(value) : null;
      return { kind: "node", runId, nodeId, iter };
    }
    return { kind: "run", runId };
  }
  if (link.startsWith("#/ext/")) return { kind: "extension", href: link };
  if (link.startsWith("/runs/")) {
    const [path, query = ""] = link.split("?");
    const runId = decode(path.slice("/runs/".length).split("/")[0] ?? "");
    return { kind: "review", runId, file: new URLSearchParams(query).get("file"), href: link };
  }
  if (link.startsWith("#")) return { kind: "anchor", anchor: decode(link.slice(1)) };
  return { kind: "path", path: link };
}

/**
 * The words of an entry's link, as the panel shows them: what it opens, then where.
 * `runName` names a Run the app knows; otherwise its id is shown.
 */
export function guideLinkLabel(
  target: GuideTarget,
  runName: (runId: string) => string | null,
): { kind: string; where: string } {
  const run = (id: string) => runName(id) ?? id;
  switch (target.kind) {
    case "run":
      return { kind: "Run", where: run(target.runId) };
    case "node":
      return { kind: "Run", where: `${run(target.runId)} › ${target.nodeId}` };
    case "artifact":
      return { kind: "Artifact", where: `${target.nodeId} › ${target.port}` };
    case "review":
      return { kind: "Review", where: target.file ?? run(target.runId) };
    case "extension":
      return { kind: "Page", where: target.href.slice("#/ext/".length) };
    case "anchor":
      return { kind: "#", where: `#${target.anchor}` };
    case "path":
      return { kind: "#", where: target.path };
  }
}
