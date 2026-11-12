import { describe, expect, it } from "vitest";
import type { Project } from "../../workspace/model/project";
import {
  ambiguousRepository,
  initialProject,
  launchesInProject,
  memberNames,
  NO_PROJECT,
  offeredProjects,
  offersNoProject,
  projectRepoGroups,
  projectsHolding,
  projectSecondaries,
  repoCount,
} from "./launchProject";
import { UNKNOWN_RIGHTS, type RunCreationRights } from "./runCreationRights";

const checkout: Project = {
  id: "p1",
  name: "Checkout",
  members: ["/code/shop-web", "/code/shop-api", "/code/design-system"],
  primary_repo: "/code/shop-web",
};
const mobile: Project = {
  id: "p2",
  name: "Mobile",
  members: ["/code/shop-app", "/code/design-system"],
};

const known = (patch: Partial<RunCreationRights>): RunCreationRights => ({
  ...UNKNOWN_RIGHTS,
  known: true,
  ...patch,
});

describe("the Projet of a launch (ADR-0087)", () => {
  it("offers every Projet by name, or only those the Droits allow", () => {
    expect(offeredProjects(UNKNOWN_RIGHTS, [mobile, checkout]).map((p) => p.id)).toEqual([
      "p1",
      "p2",
    ]);
    const restricted = known({ allowedProjects: [mobile], hiddenProjects: [checkout] });
    expect(offeredProjects(restricted, [mobile, checkout]).map((p) => p.id)).toEqual(["p2"]);
  });

  it("opens on the last Projet chosen while it is offered, else the first one", () => {
    const offered = [checkout, mobile];
    expect(initialProject(offered, UNKNOWN_RIGHTS, "p2")).toBe("p2");
    expect(initialProject(offered, UNKNOWN_RIGHTS, "gone")).toBe("p1");
    expect(initialProject(offered, UNKNOWN_RIGHTS, null)).toBe("p1");
    expect(initialProject([], UNKNOWN_RIGHTS, null)).toBeNull();
  });

  it("reopens on « No project » only when the Droits allow a Run in no Projet", () => {
    const offered = [checkout];
    expect(initialProject(offered, UNKNOWN_RIGHTS, NO_PROJECT)).toBeNull();
    expect(initialProject(offered, known({ outsideProjects: false }), NO_PROJECT)).toBe("p1");
  });

  it("opens on no Projet rather than the first one when the field only searches", () => {
    const offered = [checkout, mobile];
    expect(initialProject(offered, known({ canBrowse: false }), null, true)).toBeNull();
    expect(initialProject(offered, known({ canBrowse: false }), "p2", true)).toBe("p2");
    expect(initialProject(offered, known({ canBrowse: false }), "gone", true)).toBeNull();
  });

  it("offers « No project » only to whoever may read the disk", () => {
    expect(offersNoProject(known({ outsideProjects: true, canBrowse: true }))).toBe(true);
    expect(offersNoProject(known({ outsideProjects: true, canBrowse: false }))).toBe(false);
    expect(offersNoProject(known({ outsideProjects: false, canBrowse: true }))).toBe(false);
  });

  it("lists the repositories of the chosen Projet, else of every Projet offered, under each", () => {
    expect(projectRepoGroups([checkout, mobile], null)).toEqual([
      { label: "Checkout", projectId: "p1", repos: ["/code/shop-web", "/code/shop-api", "/code/design-system"] },
      { label: "Mobile", projectId: "p2", repos: ["/code/shop-app", "/code/design-system"] },
    ]);
    expect(projectRepoGroups([checkout, mobile], mobile)).toEqual([
      { label: "Mobile", projectId: "p2", repos: ["/code/shop-app", "/code/design-system"] },
    ]);
    const empty: Project = { id: "p3", name: "Empty", members: [] };
    expect(projectRepoGroups([empty], null)).toEqual([]);
  });

  it("names the Projets holding a repository, as the daemon refuses it in none", () => {
    expect(projectsHolding([checkout, mobile], "/code/design-system/").map((p) => p.id)).toEqual(["p1", "p2"]);
    expect(projectsHolding([checkout, mobile], "/code/shop-app").map((p) => p.id)).toEqual(["p2"]);
    expect(projectsHolding([checkout, mobile], "  ")).toEqual([]);
    expect(ambiguousRepository(["Payments", "Risk"])).toBe("This repository is in Payments and Risk: pick its Project.");
    expect(ambiguousRepository(["A", "B", "C"])).toBe("This repository is in A, B and C: pick its Project.");
    // By name, whatever their order, as the daemon's 400 says it.
    expect(ambiguousRepository(["Payments", "ledger", "Audit"])).toBe(
      "This repository is in Audit, ledger and Payments: pick its Project.",
    );
  });

  it("offers the Projet's other repositories as secondaries", () => {
    expect(projectSecondaries(checkout, "/code/shop-web")).toEqual([
      "/code/shop-api",
      "/code/design-system",
    ]);
    expect(projectSecondaries(checkout, "/code/shop-api")).toEqual([
      "/code/shop-web",
      "/code/design-system",
    ]);
    expect(projectSecondaries(null, "/code/shop-web")).toEqual([]);
  });

  it("launches in a Projet on one of its repositories only", () => {
    expect(launchesInProject(checkout, "/code/shop-api")).toBe(true);
    expect(launchesInProject(checkout, "/code/shop-api/")).toBe(true);
    expect(launchesInProject(checkout, "/code/elsewhere")).toBe(false);
    expect(launchesInProject(null, "/code/elsewhere")).toBe(true);
  });

  it("names a Projet's repositories as the menu shows them", () => {
    expect(repoCount(checkout)).toBe("3 repos");
    expect(repoCount({ ...mobile, members: ["/x"] })).toBe("1 repo");
    expect(memberNames(mobile)).toBe("shop-app · design-system");
  });

  it("names the default primary repository first", () => {
    const sorted = { ...checkout, members: ["/code/design-system", "/code/shop-api", "/code/shop-web"] };
    expect(memberNames(sorted)).toBe("shop-web · design-system · shop-api");
  });
});
