// The Review page's file tree (CORE-84): the diff's paths as nested folders,
// the way a forge's "Files changed" shows them. Folders come before files at
// each level (first-seen order), files in natural order (`m2` before `m10`,
// where the diff sorts them as text), and a chain of folders that each hold a
// single folder reads as one row (`src/a/b`).
//
// A folder is keyed by its full path — for a compressed row, the path of its
// last segment — and that key is what the collapse state holds.

export type TreeNode =
  | { kind: "dir"; path: string; name: string; children: TreeNode[] }
  | { kind: "file"; path: string; name: string; index: number };

type DirNode = Extract<TreeNode, { kind: "dir" }>;

interface Building {
  dirs: Map<string, Building>;
  files: { path: string; name: string; index: number }[];
}

/** The tree of `paths`; each file node carries its index in `paths`. */
export function buildFileTree(paths: string[]): TreeNode[] {
  const root: Building = { dirs: new Map(), files: [] };
  paths.forEach((path, index) => {
    const parts = path.split("/");
    let at = root;
    for (const part of parts.slice(0, -1)) {
      let next = at.dirs.get(part);
      if (!next) {
        next = { dirs: new Map(), files: [] };
        at.dirs.set(part, next);
      }
      at = next;
    }
    at.files.push({ path, name: parts[parts.length - 1], index });
  });
  return toNodes(root, "");
}

const NATURAL = new Intl.Collator(undefined, { numeric: true });

function toNodes(b: Building, prefix: string): TreeNode[] {
  const dirs: TreeNode[] = [...b.dirs].map(([part, child]) => {
    let name = part;
    let path = prefix + part;
    let at = child;
    while (at.files.length === 0 && at.dirs.size === 1) {
      const [[next, only]] = [...at.dirs];
      name += `/${next}`;
      path += `/${next}`;
      at = only;
    }
    return { kind: "dir", path, name, children: toNodes(at, `${path}/`) };
  });
  const files: TreeNode[] = [...b.files]
    .sort((x, y) => NATURAL.compare(x.name, y.name))
    .map((f) => ({ kind: "file", ...f }));
  return [...dirs, ...files];
}

/** `items` in the order the tree shows them (depth-first, folders first). */
export function treeOrder<T>(items: T[], pathOf: (item: T) => string): T[] {
  const out: T[] = [];
  const walk = (nodes: TreeNode[]) => {
    for (const n of nodes) {
      if (n.kind === "dir") walk(n.children);
      else out.push(items[n.index]);
    }
  };
  walk(buildFileTree(items.map(pathOf)));
  return out;
}

export interface TreeRow {
  node: TreeNode;
  depth: number;
  /** A folder row: shown open (its content follows). */
  open: boolean;
}

/**
 * The rows the list shows. Without a filter, a collapsed folder keeps its row
 * and hides its content. With one, the tree keeps its shape but holds only the
 * files whose path contains it (case-insensitive) and the folders leading to
 * them, all open — the collapse state is ignored, not lost.
 */
/** The filter's rule: the path contains it, case-insensitively (an empty filter matches everything). */
export function matchesFilter(path: string, filter: string): boolean {
  const q = filter.trim().toLowerCase();
  return !q || path.toLowerCase().includes(q);
}

export function visibleRows(tree: TreeNode[], collapsed: ReadonlySet<string>, filter: string): TreeRow[] {
  const q = filter.trim().toLowerCase();
  const rows: TreeRow[] = [];
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const n of nodes) {
      if (n.kind === "file") {
        if (matchesFilter(n.path, q)) rows.push({ node: n, depth, open: false });
        continue;
      }
      if (q && !hasMatch(n, q)) continue;
      const open = q !== "" || !collapsed.has(n.path);
      rows.push({ node: n, depth, open });
      if (open) walk(n.children, depth + 1);
    }
  };
  walk(tree, 0);
  return rows;
}

function hasMatch(dir: DirNode, q: string): boolean {
  return dir.children.some((c) => (c.kind === "file" ? matchesFilter(c.path, q) : hasMatch(c, q)));
}

/** The keys of the folders holding `path`, outermost first ([] when unknown or at the root). */
export function ancestorDirs(tree: TreeNode[], path: string): string[] {
  const out: string[] = [];
  let nodes = tree;
  for (;;) {
    const dir = nodes.find((n): n is DirNode => n.kind === "dir" && path.startsWith(`${n.path}/`));
    if (!dir) break;
    out.push(dir.path);
    nodes = dir.children;
  }
  return nodes.some((n) => n.kind === "file" && n.path === path) ? out : [];
}

/** Every folder key, in tree order. */
export function allDirs(tree: TreeNode[]): string[] {
  return tree.flatMap((n) => (n.kind === "dir" ? [n.path, ...allDirs(n.children)] : []));
}

/** `countOf` summed over every file a folder holds, at any depth. */
export function dirCount(dir: DirNode, countOf: (path: string) => number): number {
  return dir.children.reduce((sum, c) => sum + (c.kind === "file" ? countOf(c.path) : dirCount(c, countOf)), 0);
}
