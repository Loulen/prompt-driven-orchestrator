import { describe, it, expect } from "vitest";
import { groupByProject, type ProjectRef } from "./groupByRepo";

interface Row {
  id: string;
  repo: string | null;
}

const repoOf = (r: Row) => r.repo;

/** Build a verbatim `path → ProjectRef | null` lookup from a members map. */
function projectOfFrom(
  members: Record<string, ProjectRef>,
): (path: string) => ProjectRef | null {
  return (path) => members[path] ?? null;
}

const NO_PROJECTS = projectOfFrom({});

describe("groupByProject", () => {
  it("returns null for an empty list", () => {
    expect(groupByProject([] as Row[], repoOf, NO_PROJECTS)).toBeNull();
  });

  it("without any Projet, behaves exactly like the #258 per-path grouping", () => {
    // AC: sans Projet, les listes se groupent exactement comme aujourd'hui.
    const rows: Row[] = [
      { id: "a", repo: "/repos/foo" },
      { id: "b", repo: "/repos/foo" },
    ];
    // One distinct path, no Projet → flat.
    expect(groupByProject(rows, repoOf, NO_PROJECTS)).toBeNull();
  });

  it("groups ≥2 distinct paths (no Projet), derived basename labels", () => {
    // AC vitest: libellé dérivé quand il n'y a pas de Projet.
    const rows: Row[] = [
      { id: "z1", repo: "/repos/zebra" },
      { id: "a1", repo: "/repos/alpha" },
      { id: "z2", repo: "/repos/zebra" },
    ];
    const groups = groupByProject(rows, repoOf, NO_PROJECTS);
    expect(groups).not.toBeNull();
    expect(groups!.map((g) => g.label)).toEqual(["alpha", "zebra"]);
    expect(groups!.every((g) => g.kind === "path")).toBe(true);
    // Items preserved in input order within each group.
    expect(groups!.find((g) => g.label === "zebra")!.items.map((i) => i.id)).toEqual([
      "z1",
      "z2",
    ]);
  });

  it("disambiguates colliding basenames on path groups (minimal suffix)", () => {
    const rows: Row[] = [
      { id: "1", repo: "/a/app" },
      { id: "2", repo: "/b/app" },
    ];
    const groups = groupByProject(rows, repoOf, NO_PROJECTS)!;
    expect(groups.map((g) => g.label).sort()).toEqual(["a/app", "b/app"]);
  });

  it("collapses a Projet's member paths under its name (single Projet still shows)", () => {
    // FP: nommer un Projet et y rattacher les deux dépôts → les deux se rangent
    // sous ce nom. A single Projet over the only two repos still renders (the
    // "any group is a Projet" clause), it does not flatten.
    const front = "/repos/front";
    const back = "/repos/back";
    const projectOf = projectOfFrom({
      [front]: { id: "p1", name: "MyProduct" },
      [back]: { id: "p1", name: "MyProduct" },
    });
    const rows: Row[] = [
      { id: "f", repo: front },
      { id: "b", repo: back },
    ];
    const groups = groupByProject(rows, repoOf, projectOf);
    expect(groups).not.toBeNull();
    expect(groups!).toHaveLength(1);
    expect(groups![0].kind).toBe("project");
    expect(groups![0].label).toBe("MyProduct");
    // Both repos' rows are under the one name.
    expect(groups![0].items.map((i) => i.id)).toEqual(["f", "b"]);
    // The hover title exposes the member paths.
    expect(groups![0].title).toBe([back, front].sort().join(", "));
  });

  it("counts a Projet as one unit — the threshold moved onto projects, not paths", () => {
    // A Projet grouping two repos + one loose repo = 2 groups → grouped, with the
    // Projet collapsing its two member paths into a single header.
    const front = "/repos/front";
    const back = "/repos/back";
    const other = "/repos/other";
    const projectOf = projectOfFrom({
      [front]: { id: "p1", name: "Product" },
      [back]: { id: "p1", name: "Product" },
    });
    const rows: Row[] = [
      { id: "f", repo: front },
      { id: "b", repo: back },
      { id: "o", repo: other },
    ];
    const groups = groupByProject(rows, repoOf, projectOf)!;
    expect(groups).toHaveLength(2);
    const project = groups.find((g) => g.kind === "project")!;
    expect(project.label).toBe("Product");
    expect(project.items.map((i) => i.id)).toEqual(["f", "b"]);
    const path = groups.find((g) => g.kind === "path")!;
    expect(path.label).toBe("other");
    expect(path.items.map((i) => i.id)).toEqual(["o"]);
  });

  it("a Projet whose members are absent from the list does not force grouping", () => {
    // projectOf returns a ref only for member paths actually present. A single
    // loose repo whose path is in no Projet stays flat.
    const rows: Row[] = [{ id: "x", repo: "/repos/solo" }];
    const projectOf = projectOfFrom({
      "/repos/elsewhere": { id: "p9", name: "Elsewhere" },
    });
    expect(groupByProject(rows, repoOf, projectOf)).toBeNull();
  });

  it("compares membership verbatim — a trailing slash is a different, unowned path", () => {
    // ADR-0033: two spellings are two paths. `/repos/front/` is not the member
    // `/repos/front`, so it falls back to a path group.
    const projectOf = projectOfFrom({
      "/repos/front": { id: "p1", name: "Product" },
    });
    const rows: Row[] = [
      { id: "a", repo: "/repos/front" },
      { id: "b", repo: "/repos/front/" },
    ];
    const groups = groupByProject(rows, repoOf, projectOf)!;
    expect(groups).toHaveLength(2);
    expect(groups.some((g) => g.kind === "project" && g.label === "Product")).toBe(true);
    expect(groups.some((g) => g.kind === "path")).toBe(true);
  });

  it("orders groups by label, tie-broken by key; items keep input order", () => {
    const projectOf = projectOfFrom({
      "/repos/aaa": { id: "pz", name: "Zeta" },
    });
    const rows: Row[] = [
      { id: "1", repo: "/repos/aaa" }, // Projet "Zeta"
      { id: "2", repo: "/repos/mmm" }, // path "mmm"
    ];
    const groups = groupByProject(rows, repoOf, projectOf)!;
    // "Zeta" > "mmm"? Code-unit: 'Z'(90) < 'm'(109), so "Zeta" sorts first.
    expect(groups.map((g) => g.label)).toEqual(["Zeta", "mmm"]);
  });
});
