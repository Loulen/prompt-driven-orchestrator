import { describe, expect, it } from "vitest";
import type { Project } from "./project";
import {
  alsoIn,
  clonesFolderTarget,
  newRepositoryTarget,
  projectsEntryOf,
  recentToAdd,
  repoCount,
  repositoriesFromMyProjects,
  runCount,
  runCountByProject,
  sortedProjects,
} from "./projectsSettings";

const project = (id: string, name: string, members: string[] = []): Project => ({ id, name, members });

describe("projectsSettings", () => {
  it("lists the Projets by name", () => {
    const list = sortedProjects([project("b", "Platform"), project("a", "checkout"), project("c", "Mobile")]);
    expect(list.map((p) => p.name)).toEqual(["checkout", "Mobile", "Platform"]);
  });

  it("counts the Runs recorded in each Projet", () => {
    const counts = runCountByProject([
      { project_id: "p1" },
      { project_id: "p1" },
      { project_id: "p2" },
      {},
      { project_id: null },
    ]);
    expect(counts.get("p1")).toBe(2);
    expect(counts.get("p2")).toBe(1);
    expect(counts.size).toBe(2);
  });

  it("names the other Projets holding a repository", () => {
    const projects = [
      project("p1", "Checkout", ["/r/api", "/r/web"]),
      project("p2", "Platform", ["/r/api"]),
      project("p3", "Mobile", ["/r/api", "/r/ds"]),
    ];
    expect(alsoIn(projects, "p1", "/r/api")).toEqual(["Mobile", "Platform"]);
    expect(alsoIn(projects, "p1", "/r/web")).toEqual([]);
  });

  it("words the counts", () => {
    expect(repoCount(1)).toBe("1 repo");
    expect(repoCount(3)).toBe("3 repos");
    expect(runCount(0)).toBe("0 runs");
    expect(runCount(1)).toBe("1 run");
  });

  it("offers the recent repositories the Projet does not hold", () => {
    expect(recentToAdd(["/r/a", "/r/b", "/r/c"], project("p", "P", ["/r/b"]))).toEqual(["/r/a", "/r/c"]);
    expect(recentToAdd(["/r/a", "/r/c"], project("p", "P"), ["/r/c"])).toEqual(["/r/a"]);
  });

  it("offers the repositories of the other Projets whose repositories may be read, each once", () => {
    const payments = project("p", "Payments", ["/c/payments-api"]);
    const projects = [
      payments,
      project("r", "Risk", ["/c/risk-models", "/c/payments-api"]),
      project("q", "Quotes", ["/c/quotes-web", "/c/risk-models"]),
      project("s", "Secret", ["/c/secret"]),
    ];
    expect(repositoriesFromMyProjects(projects, payments, new Set(["p", "q", "r"]))).toEqual([
      { path: "/c/quotes-web", projectName: "Quotes" },
      { path: "/c/risk-models", projectName: "Quotes" },
    ]);
    expect(repositoriesFromMyProjects(projects, payments, new Set())).toEqual([]);
  });

  it("cuts the folder of a new repository into its parent and its name", () => {
    expect(newRepositoryTarget("/home/me/code/widgets", null)).toEqual({ parent: "/home/me/code", name: "widgets" });
    expect(newRepositoryTarget(" ~/code/widgets/ ", "/home/me/")).toEqual({ parent: "/home/me/code", name: "widgets" });
    expect(newRepositoryTarget("/widgets", null)).toEqual({ parent: "/", name: "widgets" });
    expect(newRepositoryTarget("~/x", null)).toBeNull();
    expect(newRepositoryTarget("code/widgets", "/home/me")).toBeNull();
    expect(newRepositoryTarget("/", null)).toBeNull();
    expect(newRepositoryTarget("/home/me/..", null)).toBeNull();
    expect(newRepositoryTarget("", "/home/me")).toBeNull();
  });

  it("takes a name alone for a new repository in the clones folder", () => {
    expect(clonesFolderTarget(" ledger ")).toEqual({ parent: null, name: "ledger" });
    for (const typed of ["", " ", ".", "..", "a/b", "~/x", "a\\b"]) expect(clonesFolderTarget(typed)).toBeNull();
  });
});

describe("projectsEntryOf", () => {
  it("opens a Projet's group on the Projet, a path group on a new one holding its repository", () => {
    expect(projectsEntryOf({ kind: "project", key: "project:p1", repoPath: "", label: "Checkout" })).toEqual({
      projectId: "p1",
    });
    expect(projectsEntryOf({ kind: "path", key: "/r/web", repoPath: "/r/web", label: "web" })).toEqual({
      draft: { name: "web", members: ["/r/web"] },
    });
  });
});
