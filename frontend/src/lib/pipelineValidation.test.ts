import { describe, it, expect } from "vitest";
import type { PipelineListEntry, Project } from "../types";
import {
  isOffered,
  pipelineSections,
  projectOfRepo,
  validationFromChoice,
  validationLabel,
  validationOf,
  validationSummary,
  validationTooltip,
} from "./pipelineValidation";

const p = (id: string, validation?: PipelineListEntry["validation"]): PipelineListEntry => ({
  id,
  name: id,
  scope: "instance",
  path: `/x/${id}.yaml`,
  node_count: 2,
  modified: null,
  variables: {},
  validation,
});

const alpha: Project = { id: "a", name: "Alpha", members: ["/repos/front", "/repos/back"] };
const bravo: Project = { id: "b", name: "Bravo", members: ["/repos/other"] };

describe("pipelineValidation (#974)", () => {
  it("reads a Pipeline without validation as validated for all Projets", () => {
    expect(validationOf(p("legacy"))).toEqual({ kind: "all" });
  });

  it("finds the Projet of a repository verbatim", () => {
    expect(projectOfRepo([alpha, bravo], "/repos/back")?.id).toBe("a");
    expect(projectOfRepo([alpha, bravo], " /repos/back ")?.id).toBe("a");
    expect(projectOfRepo([alpha, bravo], "/repos/back/")).toBeNull();
    expect(projectOfRepo([alpha, bravo], "")).toBeNull();
  });

  it("sorts Pipelines into the Projet's, the global ones and the test ones", () => {
    const list = [
      p("g1", { kind: "all" }),
      p("legacy"),
      p("t1", { kind: "none" }),
      p("pa", { kind: "projects", project_ids: ["a"] }),
      p("pb", { kind: "projects", project_ids: ["b"] }),
      p("pab", { kind: "projects", project_ids: ["b", "a"] }),
    ];
    const ids = (xs: PipelineListEntry[]) => xs.map((x) => x.id);

    const forAlpha = pipelineSections(list, "a");
    expect(ids(forAlpha.project)).toEqual(["pa", "pab"]);
    expect(ids(forAlpha.global)).toEqual(["g1", "legacy"]);
    expect(ids(forAlpha.test)).toEqual(["t1"]);

    const noProject = pipelineSections(list, null);
    expect(noProject.project).toEqual([]);
    expect(ids(noProject.global)).toEqual(["g1", "legacy"]);
    expect(ids(noProject.test)).toEqual(["t1"]);
  });

  it("does not offer a Pipeline validated for other Projets only", () => {
    const pb = p("pb", { kind: "projects", project_ids: ["b"] });
    expect(isOffered(pb, "a")).toBe(false);
    expect(isOffered(pb, null)).toBe(false);
    expect(isOffered(pb, "b")).toBe(true);
    expect(isOffered(p("t", { kind: "none" }), "a")).toBe(true);
  });

  it("summarises for the indicator, ignoring Projets that are gone", () => {
    expect(validationSummary({ kind: "all" }, [alpha])).toEqual({ all: true, count: 0, names: [] });
    const s = validationSummary({ kind: "projects", project_ids: ["a", "zz"] }, [alpha]);
    expect(s).toEqual({ all: false, count: 1, names: ["Alpha"] });
    expect(validationTooltip(s)).toBe("Validated for: Alpha");
    expect(validationTooltip(validationSummary({ kind: "none" }, []))).toMatch(/test pipeline/i);
  });

  it("builds the request from what the modal has ticked", () => {
    expect(validationFromChoice(false, [])).toEqual({ kind: "none" });
    expect(validationFromChoice(false, ["a", "b"])).toEqual({
      kind: "projects",
      project_ids: ["a", "b"],
    });
  });
});

describe("pipelineValidation — validated for all, listed first for Projets (#992)", () => {
  const ids = (xs: PipelineListEntry[]) => xs.map((x) => x.id);
  const list = [
    p("g", { kind: "all" }),
    p("ga", { kind: "all", project_ids: ["a"] }),
    p("gab", { kind: "all", project_ids: ["b", "a"] }),
    p("pa", { kind: "projects", project_ids: ["a"] }),
  ];

  it("puts it once, in the section of a Projet it lists", () => {
    const forAlpha = pipelineSections(list, "a");
    expect(ids(forAlpha.project)).toEqual(["ga", "gab", "pa"]);
    expect(ids(forAlpha.global)).toEqual(["g"]);
  });

  it("keeps it global for another Projet or a repository without one", () => {
    const forBravo = pipelineSections(list, "b");
    expect(ids(forBravo.project)).toEqual(["gab"]);
    expect(ids(forBravo.global)).toEqual(["g", "ga"]);
    const noProject = pipelineSections(list, null);
    expect(noProject.project).toEqual([]);
    expect(ids(noProject.global)).toEqual(["g", "ga", "gab"]);
  });

  it("offers it to every Projet", () => {
    const ga = p("ga", { kind: "all", project_ids: ["a"] });
    expect(isOffered(ga, "a")).toBe(true);
    expect(isOffered(ga, "b")).toBe(true);
    expect(isOffered(ga, null)).toBe(true);
  });

  it("summarises as « All +N », naming the live Projets only", () => {
    const s = validationSummary({ kind: "all", project_ids: ["a", "zz", "b"] }, [alpha, bravo]);
    expect(s).toEqual({ all: true, count: 2, names: ["Alpha", "Bravo"] });
    expect(validationLabel(s)).toBe("All +2");
    expect(validationTooltip(s)).toBe(
      "Validated for all projects · listed first for: Alpha, Bravo",
    );
    const gone = validationSummary({ kind: "all", project_ids: ["zz"] }, [alpha]);
    expect(validationLabel(gone)).toBe("All");
    expect(validationTooltip(gone)).toBe("Validated for all projects (current and future)");
    expect(validationLabel(validationSummary({ kind: "projects", project_ids: ["a"] }, [alpha]))).toBe(
      "1",
    );
  });

  it("keeps the ticked Projets under « All » in the request", () => {
    expect(validationFromChoice(true, ["a", "b"])).toEqual({ kind: "all", project_ids: ["a", "b"] });
    expect(validationFromChoice(true, [])).toEqual({ kind: "all" });
  });
});
