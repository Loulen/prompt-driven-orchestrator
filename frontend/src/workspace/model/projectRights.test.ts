import { describe, expect, it } from "vitest";
import type { Decision } from "../../shared/authorization/api";
import type { Project } from "./project";
import { projectQuestions, projectRights } from "./projectRights";

const allow: Decision = { decision: "allow" };
const deny: Decision = { decision: "deny", reason: "no" };
const projects: Project[] = [
  { id: "p1", name: "Checkout", members: [] },
  { id: "p2", name: "Mobile", members: [] },
];

describe("projectRights", () => {
  it("asks about the creations, then each Projet's update and delete", () => {
    expect(projectQuestions(projects)).toEqual([
      { resource: "project", verb: "create" },
      { resource: "repository", verb: "read" },
      { resource: "repository", verb: "create" },
      { resource: "project", verb: "update", target: "p1" },
      { resource: "project", verb: "delete", target: "p1" },
      { resource: "project", verb: "update", target: "p2" },
      { resource: "project", verb: "delete", target: "p2" },
    ]);
  });

  it("reads a Projet the caller may not update as read-only", () => {
    const rights = projectRights(projects, [deny, allow, deny, allow, allow, deny, deny]);
    expect(rights.known).toBe(true);
    expect(rights.canCreate).toBe(false);
    expect(rights.canBrowse).toBe(true);
    expect(rights.canCreateRepository).toBe(false);
    expect([...rights.readOnly]).toEqual(["p2"]);
    expect([...rights.undeletable]).toEqual(["p2"]);
  });
});
