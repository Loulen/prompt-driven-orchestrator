import { describe, expect, it } from "vitest";
import type { Project } from "../../workspace/model/project";
import {
  initialProject,
  launchesInProject,
  memberNames,
  NO_PROJECT,
  offeredProjects,
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
