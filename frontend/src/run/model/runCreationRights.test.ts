import { describe, expect, it } from "vitest";
import type { Project } from "../../workspace/model/project";
import {
  canCreateOutsideProjects,
  canCreateSomeRun,
  hiddenProjectsNote,
  isRestricted,
  prefillRepo,
  repoGroups,
  runCreationQuestions,
  runCreationRights,
  searchesOnly,
  UNKNOWN_RIGHTS,
  whyNoRunAnywhere,
} from "./runCreationRights";

function project(id: string, name: string, members: string[]): Project {
  return { id, name, members } as unknown as Project;
}

const PDO = project("p1", "PDO", ["/home/bob/code/PDO-v2", "/home/bob/code/pdo-docs"]);
const EXT = project("p2", "pdo-widgets", ["/home/bob/code/pdo-widgets"]);
const SECRET = project("p3", "Secret", ["/home/bob/code/secret"]);
const ALLOW = { decision: "allow" } as const;
const DENY = { decision: "deny", reason: "no" } as const;

describe("the rights to create a Run (CORE-5)", () => {
  it("asks about the disk, a Run in no Projet, then a Run in each Projet", () => {
    expect(runCreationQuestions([PDO, EXT])).toEqual([
      { resource: "repository", verb: "read" },
      { resource: "run", verb: "create", projects: [] },
      { resource: "run", verb: "create", projects: ["p1"] },
      { resource: "run", verb: "create", projects: ["p2"] },
    ]);
  });

  it("reads the answers in the order asked", () => {
    const rights = runCreationRights([PDO, EXT, SECRET], [DENY, DENY, ALLOW, ALLOW, DENY]);
    expect(rights.known).toBe(true);
    expect(rights.canBrowse).toBe(false);
    expect(rights.outsideProjects).toBe(false);
    expect(rights.allowedProjects.map((p) => p.id)).toEqual(["p1", "p2"]);
    expect(rights.hiddenProjects.map((p) => p.id)).toEqual(["p3"]);
    expect(isRestricted(rights)).toBe(true);
    expect(canCreateSomeRun(rights)).toBe(true);
    expect(hiddenProjectsNote(rights)).toBe("1 Project(s) hidden: you can't create Runs there.");
  });

  it("without any refusal the form is the one without extension", () => {
    const rights = runCreationRights([PDO], [ALLOW, ALLOW, ALLOW]);
    expect(isRestricted(rights)).toBe(false);
    expect(hiddenProjectsNote(rights)).toBeNull();
    expect(isRestricted(UNKNOWN_RIGHTS)).toBe(false);
    expect(canCreateSomeRun(UNKNOWN_RIGHTS)).toBe(true);
  });

  it("a Reader everywhere may create no Run", () => {
    const rights = runCreationRights([PDO, EXT], [DENY, DENY, DENY, DENY]);
    expect(canCreateSomeRun(rights)).toBe(false);
    expect(hiddenProjectsNote(rights)).toBe("2 Project(s) hidden: you can't create Runs there.");
  });

  it("groups the allowed repositories by Projet, then the others when allowed", () => {
    const rights = runCreationRights([EXT, PDO, SECRET], [DENY, DENY, ALLOW, ALLOW, DENY]);
    const recents = ["/home/bob/code/PDO-v2", "/tmp/scratch"];
    expect(repoGroups(rights, recents, [EXT, PDO, SECRET])).toEqual([
      { label: "PDO", repos: ["/home/bob/code/PDO-v2", "/home/bob/code/pdo-docs"], projectId: "p1" },
      { label: "pdo-widgets", repos: ["/home/bob/code/pdo-widgets"], projectId: "p2" },
    ]);
    const outside = { ...rights, outsideProjects: true, canBrowse: true };
    expect(repoGroups(outside, recents, [EXT, PDO, SECRET]).at(-1)).toEqual({
      label: "Other",
      repos: ["/tmp/scratch"],
    });
    // A repository of no Projet is read from the disk: without it, no « Other ».
    const noDisk = { ...rights, outsideProjects: true };
    expect(repoGroups(noDisk, recents, [EXT, PDO, SECRET]).map((g) => g.label)).toEqual(["PDO", "pdo-widgets"]);
  });

  it("searches only the Projets' repositories without reading the disk, and creates no Run in none", () => {
    const noDisk = runCreationRights([PDO], [DENY, ALLOW, ALLOW]);
    expect(searchesOnly(noDisk)).toBe(true);
    expect(canCreateOutsideProjects(noDisk)).toBe(false);
    expect(searchesOnly(UNKNOWN_RIGHTS)).toBe(false);
    expect(searchesOnly(runCreationRights([PDO], [ALLOW, ALLOW, ALLOW]))).toBe(false);
    // No Projet allows a Run and the disk may not be read: no Run anywhere, saying why.
    const nowhere = runCreationRights([PDO], [{ decision: "deny", reason: "no", sentence: "Ask for the disk." }, ALLOW, DENY]);
    expect(canCreateSomeRun(nowhere)).toBe(false);
    expect(whyNoRunAnywhere(nowhere)).toBe("Ask for the disk.");
  });

  describe("the repository the form opens on", () => {
    const restricted = runCreationRights([PDO, EXT, SECRET], [DENY, DENY, ALLOW, ALLOW, DENY]);

    it("is the most recent one when nothing is refused", () => {
      expect(prefillRepo(UNKNOWN_RIGHTS, ["/home/bob/code/secret", "/x"], [])).toBe("/home/bob/code/secret");
      expect(prefillRepo(UNKNOWN_RIGHTS, [], [])).toBeNull();
    });

    it("skips a recent repository of a Projet where a Run may not be created", () => {
      const recent = ["/home/bob/code/secret", "/home/bob/code/pdo-widgets"];
      expect(prefillRepo(restricted, recent, [PDO, EXT, SECRET])).toBe("/home/bob/code/pdo-widgets");
    });

    it("falls back to the first repository offered when no recent one is", () => {
      const browsing = runCreationRights([PDO, EXT, SECRET], [ALLOW, DENY, ALLOW, ALLOW, DENY]);
      expect(prefillRepo(browsing, ["/home/bob/code/secret", "/loose"], [PDO, EXT, SECRET])).toBe(
        "/home/bob/code/PDO-v2",
      );
    });

    it("is none rather than the first one offered when the field only searches", () => {
      expect(prefillRepo(restricted, ["/home/bob/code/secret", "/loose"], [PDO, EXT, SECRET])).toBeNull();
    });

    it("skips, when the field only searches, a recent repository of several Projets, unless the last Projet chosen holds it", () => {
      const BOTH = project("p4", "Ledger", ["/home/bob/code/PDO-v2"]);
      const searching = runCreationRights([PDO, EXT, BOTH], [DENY, DENY, ALLOW, ALLOW, ALLOW]);
      const recent = ["/home/bob/code/PDO-v2", "/home/bob/code/pdo-widgets"];
      // No Projet to open on: the form would open on « pick its Project ».
      expect(prefillRepo(searching, recent, [PDO, EXT, BOTH])).toBe("/home/bob/code/pdo-widgets");
      expect(prefillRepo(searching, recent, [PDO, EXT, BOTH], "p2")).toBe("/home/bob/code/pdo-widgets");
      // The form opens on a Projet holding it: no ambiguity.
      expect(prefillRepo(searching, recent, [PDO, EXT, BOTH], "p4")).toBe("/home/bob/code/PDO-v2");
      expect(prefillRepo(searching, ["/home/bob/code/PDO-v2"], [PDO, EXT, BOTH])).toBeNull();
    });

    it("is none when the form offers nothing", () => {
      const nowhere = runCreationRights([SECRET], [DENY, DENY, DENY]);
      expect(prefillRepo(nowhere, ["/home/bob/code/secret"], [SECRET])).toBeNull();
    });
  });
});
