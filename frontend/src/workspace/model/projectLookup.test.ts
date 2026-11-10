import { describe, expect, it } from "vitest";
import type { Project } from "./project";
import { recordedProject } from "./projectLookup";

const projects: Project[] = [{ id: "p1", name: "Checkout", members: ["/r/web"] }];

describe("recordedProject", () => {
  it("names the Projet recorded on a Run, none for a Run in no Projet", () => {
    const of = recordedProject(projects);
    expect(of("p1")).toEqual({ id: "p1", name: "Checkout" });
    expect(of(null)).toBeNull();
    expect(of(undefined)).toBeNull();
  });

  it("keeps a Projet since deleted, by its last name, marked so (CORE-96)", () => {
    const of = recordedProject(projects, [{ id: "gone", name: "Mobile" }]);
    expect(of("gone")).toEqual({ id: "gone", name: "Mobile (deleted)", deleted: true });
  });

  it("never reads a Projet the caller may not see as deleted", () => {
    expect(recordedProject(projects, [])("hidden")).toBeNull();
  });
});
