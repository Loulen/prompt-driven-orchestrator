import { describe, expect, it } from "vitest";
import type { Decision } from "../../shared/authorization/api";
import type { Project } from "./project";
import { projectQuestions, projectRights } from "./projectRights";

const allow: Decision = { decision: "allow" };
const deny: Decision = { decision: "deny", reason: "no", sentence: "You can't." };
const projects: Project[] = [
  { id: "p1", name: "Checkout", members: [] },
  { id: "p2", name: "Mobile", members: [] },
];

describe("projectRights", () => {
  it("asks about the creation and the disk, then per Projet its update, delete and repositories", () => {
    expect(projectQuestions(projects)).toEqual([
      { resource: "project", verb: "create" },
      { resource: "repository", verb: "read" },
      { resource: "project", verb: "update", target: "p1" },
      { resource: "project", verb: "delete", target: "p1" },
      { resource: "repository", verb: "create", projects: ["p1"] },
      { resource: "repository", verb: "read", projects: ["p1"] },
      { resource: "project", verb: "update", target: "p2" },
      { resource: "project", verb: "delete", target: "p2" },
      { resource: "repository", verb: "create", projects: ["p2"] },
      { resource: "repository", verb: "read", projects: ["p2"] },
    ]);
  });

  it("reads a Projet the caller may not update as read-only", () => {
    const rights = projectRights(projects, [deny, allow, allow, allow, allow, allow, deny, deny, deny, deny]);
    expect(rights.known).toBe(true);
    expect(rights.canCreate).toBe(false);
    expect(rights.canBrowse).toBe(true);
    expect([...rights.readOnly]).toEqual(["p2"]);
    expect([...rights.undeletable]).toEqual(["p2"]);
  });

  it("says in which Projets a repository may be created, and whose repositories may be read", () => {
    const rights = projectRights(projects, [allow, deny, allow, allow, allow, allow, allow, allow, deny, deny]);
    expect(rights.canBrowse).toBe(false);
    expect(rights.why.browse).toBe("You can't.");
    expect([...rights.why.createRepository]).toEqual([["p2", "You can't."]]);
    expect([...rights.readsRepositoriesOf]).toEqual(["p1"]);
  });
});
