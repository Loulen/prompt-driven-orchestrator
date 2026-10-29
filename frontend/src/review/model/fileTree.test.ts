import { describe, it, expect } from "vitest";
import { ancestorDirs, allDirs, buildFileTree, byNumberRuns, dirCount, treeOrder, visibleRows } from "./fileTree";
import type { TreeNode } from "./fileTree";

const PATHS = [
  "CHANGELOG.md",
  "crates/core/src/contexts/pipelines/draft.rs",
  "crates/core/src/contexts/pipelines/versions.rs",
  "crates/core/src/ws/hub.rs",
  "frontend/src/pipeline/components/Modal.tsx",
  "frontend/src/pipeline/store/draft.ts",
];

/** `dir/` or file name per row, indented by depth — the shape at a glance. */
function shape(nodes: TreeNode[], depth = 0): string[] {
  return nodes.flatMap((n) =>
    n.kind === "dir"
      ? [`${"  ".repeat(depth)}${n.name}/`, ...shape(n.children, depth + 1)]
      : [`${"  ".repeat(depth)}${n.name}`],
  );
}

describe("buildFileTree (CORE-84)", () => {
  it("nests directories, folders before files, compressing single-child chains", () => {
    expect(shape(buildFileTree(PATHS))).toEqual([
      "crates/core/src/",
      "  contexts/pipelines/",
      "    draft.rs",
      "    versions.rs",
      "  ws/",
      "    hub.rs",
      "frontend/src/pipeline/",
      "  components/",
      "    Modal.tsx",
      "  store/",
      "    draft.ts",
      "CHANGELOG.md",
    ]);
  });

  it("keys a compressed folder by its full path and indexes files in the input order", () => {
    const tree = buildFileTree(PATHS);
    const crates = tree[0];
    expect(crates).toMatchObject({ kind: "dir", path: "crates/core/src", name: "crates/core/src" });
    const pipelines = crates.kind === "dir" ? crates.children[0] : null;
    expect(pipelines).toMatchObject({ kind: "dir", path: "crates/core/src/contexts/pipelines", name: "contexts/pipelines" });
    expect(tree[2]).toEqual({ kind: "file", path: "CHANGELOG.md", name: "CHANGELOG.md", index: 0 });
  });

  it("keeps a folder holding one file and one folder uncompressed", () => {
    expect(shape(buildFileTree(["a/x.ts", "a/b/y.ts"]))).toEqual(["a/", "  b/", "    y.ts", "  x.ts"]);
  });

  it("orders the files of a folder naturally, a number read as a number", () => {
    expect(shape(buildFileTree(["d/m1.ts", "d/m10.ts", "d/m11.ts", "d/m2.ts"]))).toEqual([
      "d/",
      "  m1.ts",
      "  m2.ts",
      "  m10.ts",
      "  m11.ts",
    ]);
  });

  it("orders the folders of a level with a number read as a number", () => {
    expect(shape(buildFileTree(["m1/a.ts", "m10/a.ts", "m11/a.ts", "m2/a.ts"]))).toEqual([
      "m1/",
      "  a.ts",
      "m2/",
      "  a.ts",
      "m10/",
      "  a.ts",
      "m11/",
      "  a.ts",
    ]);
  });

  it("reads a run of digits as a number and keeps the diff's order everywhere else", () => {
    const sorted = (names: string[]) => [...names].sort(byNumberRuns);
    expect(sorted(["m10", "m2", "m1"])).toEqual(["m1", "m2", "m10"]);
    expect(sorted(["foo2", "foo.d", "foo-bar"])).toEqual(["foo-bar", "foo.d", "foo2"]);
    expect(sorted(["a", "B"])).toEqual(["B", "a"]);
    expect(sorted(["v100000000000000000000", "v99999999999999999999"])).toEqual([
      "v99999999999999999999",
      "v100000000000000000000",
    ]);
    expect(sorted(["x01", "x1", "x001"])).toEqual(["x001", "x01", "x1"]);
  });

  it("never folds a file into its folder's name", () => {
    expect(shape(buildFileTree(["a/b/only.ts"]))).toEqual(["a/b/", "  only.ts"]);
    expect(shape(buildFileTree([]))).toEqual([]);
  });
});

describe("treeOrder", () => {
  it("lists the items in the order the tree shows them", () => {
    const order = treeOrder(PATHS, (p) => p);
    expect(order).toEqual([
      "crates/core/src/contexts/pipelines/draft.rs",
      "crates/core/src/contexts/pipelines/versions.rs",
      "crates/core/src/ws/hub.rs",
      "frontend/src/pipeline/components/Modal.tsx",
      "frontend/src/pipeline/store/draft.ts",
      "CHANGELOG.md",
    ]);
  });
});

describe("visibleRows", () => {
  const tree = buildFileTree(treeOrder(PATHS, (p) => p));
  const label = (rows: ReturnType<typeof visibleRows>) =>
    rows.map((r) => `${r.depth}:${r.node.name}${r.node.kind === "dir" ? (r.open ? "/" : "/+") : ""}`);

  it("hides what a collapsed folder holds, keeps the folder row", () => {
    const rows = visibleRows(tree, new Set(["crates/core/src/contexts/pipelines", "frontend/src/pipeline"]), "");
    expect(label(rows)).toEqual([
      "0:crates/core/src/",
      "1:contexts/pipelines/+",
      "1:ws/",
      "2:hub.rs",
      "0:frontend/src/pipeline/+",
      "0:CHANGELOG.md",
    ]);
  });

  it("filters: keeps the tree, drops branches with no match and opens the ones with a match", () => {
    const rows = visibleRows(tree, new Set(["crates/core/src", "frontend/src/pipeline"]), "draft");
    expect(label(rows)).toEqual([
      "0:crates/core/src/",
      "1:contexts/pipelines/",
      "2:draft.rs",
      "0:frontend/src/pipeline/",
      "1:store/",
      "2:draft.ts",
    ]);
  });

  it("matches the filter on the whole path, case-insensitively", () => {
    const rows = visibleRows(tree, new Set(), "WS/");
    expect(label(rows)).toEqual(["0:crates/core/src/", "1:ws/", "2:hub.rs"]);
  });
});

describe("ancestorDirs / allDirs / dirCount", () => {
  const tree = buildFileTree(PATHS);

  it("lists a file's folders, outermost first, by their tree keys", () => {
    expect(ancestorDirs(tree, "crates/core/src/contexts/pipelines/draft.rs")).toEqual([
      "crates/core/src",
      "crates/core/src/contexts/pipelines",
    ]);
    expect(ancestorDirs(tree, "CHANGELOG.md")).toEqual([]);
    expect(ancestorDirs(tree, "nope.ts")).toEqual([]);
  });

  it("lists every folder key", () => {
    expect(allDirs(tree)).toEqual([
      "crates/core/src",
      "crates/core/src/contexts/pipelines",
      "crates/core/src/ws",
      "frontend/src/pipeline",
      "frontend/src/pipeline/components",
      "frontend/src/pipeline/store",
    ]);
  });

  it("sums a per-file count over everything a folder holds", () => {
    const n = new Map([
      ["crates/core/src/contexts/pipelines/draft.rs", 2],
      ["crates/core/src/contexts/pipelines/versions.rs", 1],
      ["crates/core/src/ws/hub.rs", 0],
    ]);
    const [crates, frontend] = tree;
    expect(crates.kind === "dir" && dirCount(crates, (p) => n.get(p) ?? 0)).toBe(3);
    expect(frontend.kind === "dir" && dirCount(frontend, (p) => n.get(p) ?? 0)).toBe(0);
  });
});
