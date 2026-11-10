import { describe, it, expect } from "vitest";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { Project } from "../../workspace/model/project";
import {
  defaultPipeline,
  isOffered,
  pipelineSections,
  projectOfRepo,
  validationFromChoice,
  validationOf,
  validationSummary,
  validationTooltip,
} from "./pipelineValidation";

const p = (id: string, validation: PipelineListEntry["validation"] = { kind: "all" }): PipelineListEntry => ({
  id,
  name: id,
  scope: "instance",
  path: `/x/${id}.yaml`,
  node_count: 2,
  modified: null,
  variables: {},
  prompt_required: true,
  validation,
});

const alpha: Project = { id: "a", name: "Alpha", members: ["/repos/front", "/repos/back"] };
const bravo: Project = { id: "b", name: "Bravo", members: ["/repos/other"] };
const ids = (xs: PipelineListEntry[]) => xs.map((x) => x.id);

describe("pipelineValidation", () => {
  it("reads a missing Pipeline, or a missing validation, as validated for all Projets", () => {
    expect(validationOf(undefined)).toEqual({ kind: "all" });
    expect(validationOf({} as PipelineListEntry)).toEqual({ kind: "all" });
    expect(validationOf(p("t", { kind: "none" }))).toEqual({ kind: "none" });
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

    const forAlpha = pipelineSections(list, "a");
    expect(ids(forAlpha.project)).toEqual(["pa", "pab"]);
    expect(ids(forAlpha.global)).toEqual(["g1", "legacy"]);
    expect(ids(forAlpha.test)).toEqual(["t1"]);

    const noProject = pipelineSections(list, null);
    expect(noProject.project).toEqual([]);
    expect(ids(noProject.global)).toEqual(["g1", "legacy"]);
    expect(ids(noProject.test)).toEqual(["t1"]);
  });

  it("puts a Pipeline validated for all in the Projets it is listed first for", () => {
    const list = [
      p("g1"),
      p("first", { kind: "all", project_ids: ["a"] }),
      p("pa", { kind: "projects", project_ids: ["a"] }),
    ];
    const forAlpha = pipelineSections(list, "a");
    expect(ids(forAlpha.project)).toEqual(["first", "pa"]);
    expect(ids(forAlpha.global)).toEqual(["g1"]);

    const forBravo = pipelineSections(list, "b");
    expect(ids(forBravo.project)).toEqual([]);
    expect(ids(forBravo.global)).toEqual(["g1", "first"]);
    expect(ids(pipelineSections(list, null).global)).toEqual(["g1", "first"]);

    expect(isOffered(list[1], "b")).toBe(true);
    expect(defaultPipeline(forAlpha)?.id).toBe("first");
    expect(defaultPipeline(forBravo)?.id).toBe("g1");
  });

  it("does not offer a Pipeline validated for other Projets only", () => {
    const pb = p("pb", { kind: "projects", project_ids: ["b"] });
    expect(isOffered(pb, "a")).toBe(false);
    expect(isOffered(pb, null)).toBe(false);
    expect(isOffered(pb, "b")).toBe(true);
    expect(isOffered(p("t", { kind: "none" }), "a")).toBe(true);
  });

  it("defaults to the Projet's first Pipeline, else the first global one, never a test one", () => {
    const list = [p("g1"), p("t1", { kind: "none" }), p("pa", { kind: "projects", project_ids: ["a"] })];
    expect(defaultPipeline(pipelineSections(list, "a"))?.id).toBe("pa");
    expect(defaultPipeline(pipelineSections(list, "b"))?.id).toBe("g1");
    expect(defaultPipeline(pipelineSections([p("t1", { kind: "none" })], "a"))).toBeUndefined();
  });

  it("summarises for the indicator, ignoring Projets the actor cannot read", () => {
    expect(validationSummary({ kind: "all" }, [alpha])).toEqual({ all: true, count: 0, names: [] });
    const s = validationSummary({ kind: "projects", project_ids: ["a", "zz"] }, [alpha]);
    expect(s).toEqual({ all: false, count: 1, names: ["Alpha"] });
    expect(validationTooltip(s)).toBe("Validated for: Alpha");
    expect(validationTooltip(validationSummary({ kind: "none" }, []))).toMatch(/test pipeline/i);
    expect(validationTooltip(validationSummary({ kind: "all" }, []))).toMatch(/all projects/i);

    const first = validationSummary({ kind: "all", project_ids: ["a", "zz"] }, [alpha]);
    expect(first).toEqual({ all: true, count: 0, names: ["Alpha"] });
    expect(validationTooltip(first)).toBe(
      "Validated for all projects (current and future), listed first for: Alpha",
    );
  });

  it("builds the request from what the modal has ticked", () => {
    expect(validationFromChoice(true, [])).toEqual({ kind: "all" });
    expect(validationFromChoice(true, ["a"])).toEqual({ kind: "all", project_ids: ["a"] });
    expect(validationFromChoice(false, [])).toEqual({ kind: "none" });
    expect(validationFromChoice(false, ["a", "b"])).toEqual({
      kind: "projects",
      project_ids: ["a", "b"],
    });
  });
});
