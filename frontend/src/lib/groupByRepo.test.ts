import { describe, it, expect } from "vitest";
import { groupByRepo, repoGroupLabel } from "./groupByRepo";

interface Row {
  id: string;
  repo: string | null;
}

const repoOf = (r: Row) => r.repo;

describe("groupByRepo", () => {
  it("returns null for an empty list (nothing to group)", () => {
    expect(groupByRepo([] as Row[], repoOf)).toBeNull();
  });

  it("returns null when every item shares one repo (flat, single-repo case)", () => {
    const rows: Row[] = [
      { id: "a", repo: "/repos/foo" },
      { id: "b", repo: "/repos/foo" },
    ];
    expect(groupByRepo(rows, repoOf)).toBeNull();
  });

  it("returns null when only one distinct non-empty repo exists despite null rows", () => {
    const rows: Row[] = [
      { id: "a", repo: "/repos/foo" },
      { id: "b", repo: null },
    ];
    // A single non-empty repo never trips the threshold — stays flat.
    expect(groupByRepo(rows, repoOf)).toBeNull();
  });

  it("groups into ≥2 repos in alphabetical path order, items in input order", () => {
    const rows: Row[] = [
      { id: "z1", repo: "/repos/zebra" },
      { id: "a1", repo: "/repos/alpha" },
      { id: "z2", repo: "/repos/zebra" },
      { id: "a2", repo: "/repos/alpha" },
    ];
    const groups = groupByRepo(rows, repoOf);
    expect(groups).not.toBeNull();
    expect(groups!.map((g) => g.repoPath)).toEqual([
      "/repos/alpha",
      "/repos/zebra",
    ]);
    // Within each group, input order is preserved.
    expect(groups![0].items.map((r) => r.id)).toEqual(["a1", "a2"]);
    expect(groups![1].items.map((r) => r.id)).toEqual(["z1", "z2"]);
    // Labels are basenames when there's no collision.
    expect(groups!.map((g) => g.label)).toEqual(["alpha", "zebra"]);
  });

  it("flips a single-repo list to grouped once a second repo appears (e.g. archived row)", () => {
    const flat: Row[] = [
      { id: "live", repo: "/repos/foo" },
      { id: "archived", repo: "/repos/foo" },
    ];
    expect(groupByRepo(flat, repoOf)).toBeNull();

    const mixed: Row[] = [
      ...flat,
      { id: "archived-other", repo: "/repos/bar" },
    ];
    const groups = groupByRepo(mixed, repoOf);
    expect(groups).not.toBeNull();
    expect(groups!.map((g) => g.repoPath)).toEqual([
      "/repos/bar",
      "/repos/foo",
    ]);
    expect(groups!.find((g) => g.repoPath === "/repos/foo")!.items.map((r) => r.id)).toEqual([
      "live",
      "archived",
    ]);
  });

  it("places null/empty-repo items in a single nameless bucket when grouping is active", () => {
    const rows: Row[] = [
      { id: "a", repo: "/repos/foo" },
      { id: "b", repo: "/repos/bar" },
      { id: "n1", repo: null },
      { id: "n2", repo: "" },
    ];
    const groups = groupByRepo(rows, repoOf);
    expect(groups).not.toBeNull();
    const nameless = groups!.find((g) => g.repoPath === "");
    expect(nameless).toBeDefined();
    expect(nameless!.label).toBe("");
    expect(nameless!.items.map((r) => r.id)).toEqual(["n1", "n2"]);
    // Empty key sorts first (lexicographic).
    expect(groups![0].repoPath).toBe("");
  });
});

describe("repoGroupLabel", () => {
  it("returns the bare basename when no other path collides", () => {
    expect(repoGroupLabel("/home/u/projects/maestro", ["/home/u/projects/maestro", "/other/repo"]))
      .toBe("maestro");
  });

  it("disambiguates a two-way basename collision with the minimal trailing suffix", () => {
    const all = ["/a/foo", "/b/foo"];
    expect(repoGroupLabel("/a/foo", all)).toBe("a/foo");
    expect(repoGroupLabel("/b/foo", all)).toBe("b/foo");
  });

  it("disambiguates a three-way collision, escalating k until all are unique", () => {
    // Two share the parent `a`; only the grandparent distinguishes those two.
    const all = ["/x/a/svc", "/y/a/svc", "/z/b/svc"];
    // k=2 ("a/svc","a/svc","b/svc") is not all-unique → escalate to k=3.
    expect(repoGroupLabel("/x/a/svc", all)).toBe("x/a/svc");
    expect(repoGroupLabel("/y/a/svc", all)).toBe("y/a/svc");
    // The non-colliding-at-k2 one still uses k=3 (uniform k across the group).
    expect(repoGroupLabel("/z/b/svc", all)).toBe("z/b/svc");
  });

  it("treats repeated identical paths as one (no spurious suffix)", () => {
    // Several triggers on the same repo: dedupe means no collision.
    expect(repoGroupLabel("/repos/foo", ["/repos/foo", "/repos/foo", "/repos/foo"]))
      .toBe("foo");
  });

  it("ignores leading/trailing slashes when computing segments", () => {
    expect(repoGroupLabel("/a/foo/", ["/a/foo/", "/b/foo"])).toBe("a/foo");
  });
});
