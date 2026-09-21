import type { DiffFile, DiffHunk, NodeState, RunDelivery, RunRef, RunRefs } from "../types";

// Shared helpers of the Review page (#749, ADR-0067; CONTEXT.md § "Relecture de
// diff"). Pure: no React, no fetch — so the URL contract, the pair semantics and
// the hunk re-serialisation are unit-testable, and the page file stays a view.

/**
 * The fallback default pair, used until the Run's refs are known. The daemon
 * decides the real default (`default_from`/`default_to` of `GET /runs/<id>/refs`):
 * fork → `worktree` while the Run's worktree exists (#835: commits *and*
 * uncommitted edits, so a running node needs no commit to be reviewed),
 * fork → `tip` once it is gone. `defaultPair` reads it.
 */
export const DEFAULT_FROM = "fork";
export const DEFAULT_TO = "tip";
export const WORKTREE_ID = "worktree";

/** Split | Unified, remembered on this browser (AC: "mémorisée localement"). */
export type ViewMode = "split" | "unified";
export const VIEW_KEY = "pdo.review.view";
/** File list open/closed, remembered on this browser. */
export const LIST_OPEN_KEY = "pdo.review.list";

export function readView(storage: Pick<Storage, "getItem"> = localStorage): ViewMode {
  try {
    return storage.getItem(VIEW_KEY) === "unified" ? "unified" : "split";
  } catch {
    return "split";
  }
}

export function writeView(view: ViewMode, storage: Pick<Storage, "setItem"> = localStorage): void {
  try {
    storage.setItem(VIEW_KEY, view);
  } catch {
    // Private mode / quota: the toggle still works for the session.
  }
}

export function readListOpen(storage: Pick<Storage, "getItem"> = localStorage): boolean {
  try {
    return storage.getItem(LIST_OPEN_KEY) !== "closed";
  } catch {
    return true;
  }
}

export function writeListOpen(open: boolean, storage: Pick<Storage, "setItem"> = localStorage): void {
  try {
    storage.setItem(LIST_OPEN_KEY, open ? "open" : "closed");
  } catch {
    // ignore
  }
}

/** A source → destination pair of stable ref ids. */
export interface RefPair {
  from: string;
  to: string;
}

/** The Run's default pair as the daemon states it, the fallback until then. */
export function defaultPair(refs: RunRefs | null | undefined): RefPair {
  return { from: refs?.default_from || DEFAULT_FROM, to: refs?.default_to || DEFAULT_TO };
}

/** Whether the query string names a pair at all (else the page follows the daemon's default). */
export function hasExplicitPair(search: string): boolean {
  const params = new URLSearchParams(search);
  return params.has("from") || params.has("to");
}

/** `/runs/<id>/review` — the only real URL of the app so far. */
export const REVIEW_PATH_RE = /^\/runs\/([^/]+)\/review\/?$/;

/** The Run id when `pathname` is a Review page, else null. */
export function reviewRunIdFromPath(pathname: string): string | null {
  const m = REVIEW_PATH_RE.exec(pathname);
  return m ? decodeURIComponent(m[1]) : null;
}

/**
 * The Review page URL for a pair. The default pair is left out of the query so
 * the canonical link is the short one; ids, never SHAs, so the link keeps its
 * meaning while the Run moves.
 */
export function reviewUrl(runId: string, pair?: Partial<RefPair>, defaults: RefPair = defaultPair(null)): string {
  const from = pair?.from ?? defaults.from;
  const to = pair?.to ?? defaults.to;
  const params = new URLSearchParams();
  if (from !== defaults.from || to !== defaults.to) {
    params.set("from", from);
    params.set("to", to);
  }
  const qs = params.toString();
  return `/runs/${encodeURIComponent(runId)}/review${qs ? `?${qs}` : ""}`;
}

/** The pair carried by a query string, defaults filled in. */
export function pairFromSearch(search: string, defaults: RefPair = defaultPair(null)): RefPair {
  const params = new URLSearchParams(search);
  return {
    from: params.get("from") || defaults.from,
    to: params.get("to") || defaults.to,
  };
}

export function isDefaultPair(pair: RefPair, defaults: RefPair = defaultPair(null)): boolean {
  return pair.from === defaults.from && pair.to === defaults.to;
}

/**
 * Validate a URL pair against the Run's refs. A side the Run does not know
 * falls back to its default, and `notice` says so (the page shows it once).
 */
export function reconcilePair(pair: RefPair, refs: RunRefs): { pair: RefPair; notice: string | null } {
  const known = new Set(refs.refs.map((r) => r.id));
  const bad: string[] = [];
  let { from, to } = pair;
  if (!known.has(from)) {
    bad.push(from);
    from = refs.default_from || DEFAULT_FROM;
  }
  if (!known.has(to)) {
    bad.push(to);
    to = refs.default_to || DEFAULT_TO;
  }
  return {
    pair: { from, to },
    notice: bad.length
      ? `Unknown ref${bad.length > 1 ? "s" : ""} ${bad.map((b) => `"${b}"`).join(", ")} — showing ${from} → ${to} instead.`
      : null,
  };
}

/** The delivery a pair is, when it is exactly one node's `before → after` (or `→ live`). */
export function deliveryOfPair(pair: RefPair, refs: RunRefs): RunDelivery | null {
  return (
    refs.deliveries.find(
      (d) => d.before === pair.from && (d.after === pair.to || d.live === pair.to),
    ) ?? null
  );
}

/** The pair a delivery row selects: `before → after`, or `before → live` while running. */
export function pairOfDelivery(d: RunDelivery): RefPair | null {
  const to = d.after ?? d.live;
  return to ? { from: d.before, to } : null;
}

export function refById(refs: RunRefs, id: string): RunRef | undefined {
  return refs.refs.find((r) => r.id === id);
}

/** 7-char SHA for display; a branch name is shown as is. */
export function shortSha(sha: string | null | undefined): string {
  if (!sha) return "";
  return /^[0-9a-f]{40}$/i.test(sha) ? sha.slice(0, 7) : sha;
}

/**
 * The node panel's shortcut (#749): what pair the "Review this node's delivery"
 * action opens, or null when the node has neither a recorded delivery nor a live
 * sub-worktree (pending, skipped, failed before delivering…). There is no "node
 * diff" surface: the panel only preselects a pair on the Review page.
 */
export function nodeReviewTarget(
  node: Pick<NodeState, "node_id" | "iter" | "status" | "delivery" | "isolated_worktree">,
): { pair: RefPair; label: string } | null {
  if (node.delivery) {
    return {
      pair: {
        from: `node:${node.node_id}:${node.iter}:before`,
        to: `node:${node.node_id}:${node.iter}:after`,
      },
      label: "Review this node's delivery",
    };
  }
  const live = node.status === "running" || node.status === "awaiting_user";
  if (live && node.isolated_worktree === true) {
    return {
      pair: { from: DEFAULT_TO, to: `live:${node.node_id}` },
      label: "Review live changes",
    };
  }
  return null;
}

// --- structured diff → the third-party component --------------------------

/** The path a file is keyed on: its destination, else its source. */
export function filePath(f: DiffFile): string {
  return f.new_path ?? f.old_path ?? "";
}

/** `A` / `M` / `D` / `R` — the status letter of the file list. */
export function statusLetter(f: DiffFile): "A" | "M" | "D" | "R" {
  switch (f.status) {
    case "added":
    case "copied":
      return "A";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    default:
      return "M";
  }
}

/**
 * Re-serialise one structured hunk as the unified text the diff component
 * consumes. The daemon parsed the patch once (#748); the browser never parses,
 * it only prints back what it received.
 */
export function hunkToUnified(h: DiffHunk): string {
  const head = `@@ -${h.old_start},${h.old_lines} +${h.new_start},${h.new_lines} @@${h.header ? ` ${h.header}` : ""}`;
  const body = h.lines.map((l) => (l.kind === "add" ? "+" : l.kind === "del" ? "-" : " ") + l.content);
  return [head, ...body].join("\n");
}

/**
 * Every hunk of a file, as the component's `hunks` array. The component's
 * parser (GitHub Desktop's) reads a patch, not a bare hunk: it needs the
 * `---`/`+++` header before the first `@@`, so each entry carries one.
 */
export function fileHunks(f: DiffFile): string[] {
  const header = `--- ${f.old_path ? `a/${f.old_path}` : "/dev/null"}\n+++ ${f.new_path ? `b/${f.new_path}` : "/dev/null"}\n`;
  return f.hunks.map((h) => header + hunkToUnified(h));
}

/** A rough language hint from the extension, for the component's `fileLang`. */
export function langOf(path: string | null): string {
  if (!path) return "";
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const map: Record<string, string> = {
    ts: "typescript",
    tsx: "tsx",
    js: "javascript",
    jsx: "jsx",
    mjs: "javascript",
    rs: "rust",
    py: "python",
    md: "markdown",
    json: "json",
    yaml: "yaml",
    yml: "yaml",
    toml: "toml",
    css: "css",
    html: "html",
    sh: "bash",
    bash: "bash",
    sql: "sql",
    go: "go",
    java: "java",
  };
  return map[ext] ?? "";
}

/** Files grouped by directory, in patch order, for the flat-grouped list. */
export function groupByDir(files: DiffFile[]): { dir: string; files: { file: DiffFile; index: number }[] }[] {
  const groups: { dir: string; files: { file: DiffFile; index: number }[] }[] = [];
  files.forEach((file, index) => {
    const p = filePath(file);
    const i = p.lastIndexOf("/");
    const dir = i >= 0 ? p.slice(0, i) : "";
    const last = groups[groups.length - 1];
    if (last && last.dir === dir) last.files.push({ file, index });
    else groups.push({ dir, files: [{ file, index }] });
  });
  return groups;
}

export function baseName(path: string): string {
  const i = path.lastIndexOf("/");
  return i >= 0 ? path.slice(i + 1) : path;
}

/** Five blocks: additions green, deletions red, the rest empty. */
export function miniBar(f: DiffFile, blocks = 5): ("a" | "d" | "")[] {
  if (f.binary || (f.status === "renamed" && f.additions + f.deletions === 0)) {
    return Array<"">(blocks).fill("");
  }
  const total = Math.max(1, f.additions + f.deletions);
  const a = Math.round((f.additions / total) * blocks);
  const d = blocks - a;
  return [...Array<"a">(a).fill("a"), ...Array<"d">(d).fill("d")];
}

/** Past this many files the page starts collapsed (like the Diff tab). */
export const LARGE_REVIEW_FILES = 40;
/** A single file past this many changed lines starts collapsed. */
export const LARGE_FILE_LINES = 1500;

export function defaultCollapsed(files: DiffFile[]): Set<string> {
  const init = new Set<string>();
  const large = files.length > LARGE_REVIEW_FILES;
  for (const f of files) {
    if (large || f.additions + f.deletions > LARGE_FILE_LINES) init.add(filePath(f));
  }
  return init;
}

/**
 * The signature of "what the Run has delivered so far" — every node's latest
 * `delivery.after`. When it changes while the page is open, the tip moved: the
 * page shows a banner instead of re-rendering under the reader.
 */
export function deliverySignature(nodes: Record<string, Pick<NodeState, "delivery">>): string {
  return Object.values(nodes)
    .map((n) => n.delivery?.after ?? "")
    .join("|");
}
