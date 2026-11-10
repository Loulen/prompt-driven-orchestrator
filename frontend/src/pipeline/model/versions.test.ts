import { describe, expect, it } from "vitest";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import type { OpenPipeline } from "../store/editState";
import { INITIAL_DRAFT_STATE } from "../store/draftSession";
import { changesLabel, draftLaunch, lastVersionOf, savedAgo, savedOn, shortSavedAgo, versionLabel } from "./versions";

function entry(id: string, version?: { number: number; title: string }): PipelineListEntry {
  return {
    id,
    name: id,
    scope: "instance",
    path: `/p/${id}.yaml`,
    node_count: 2,
    modified: null,
    variables: {},
    prompt_required: false,
    validation: { kind: "all" },
    ...(version ? { version } : {}),
  };
}

function tab(id: string, changes: number, patch: Partial<OpenPipeline> = {}): OpenPipeline {
  return {
    id,
    scope: "instance",
    pipeline: { name: id, variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
    dirty: changes > 0,
    externalDirty: false,
    draft: { ...INITIAL_DRAFT_STATE, synced: true, canWrite: true, changes },
    ...patch,
  };
}

describe("versions", () => {
  it("labels a version by its number, and its title when it has one", () => {
    expect(versionLabel({ number: 7, title: "Add reviewer loop" })).toBe("v7 · Add reviewer loop");
    expect(versionLabel({ number: 7, title: "v7" })).toBe("v7");
    expect(changesLabel(1)).toBe("1 change");
    expect(changesLabel(3)).toBe("3 changes");
  });

  it("reads a pipeline listed before the versions as v1", () => {
    expect(lastVersionOf([entry("flow")], "flow")).toEqual({ number: 1, title: "v1" });
    expect(lastVersionOf([entry("flow", { number: 8, title: "external edit" })], "flow").number).toBe(8);
  });

  it("asks about a launch only when the open draft has changes", () => {
    const pipelines = [entry("flow", { number: 8, title: "external edit" })];
    expect(draftLaunch("flow", [tab("flow", 3)], pipelines)).toEqual({
      tabId: "flow",
      pipelineName: "flow",
      changes: 3,
      next: 9,
      last: { number: 8, title: "external edit" },
    });
    expect(draftLaunch("flow", [tab("flow", 0)], pipelines)).toBeNull();
    expect(draftLaunch("flow", [], pipelines)).toBeNull();
    expect(draftLaunch("flow", [tab("flow", 2, { runId: "r1" })], pipelines)).toBeNull();
  });

  it("says when a version was saved", () => {
    const now = new Date("2026-10-19T15:00:00");
    expect(savedAgo("2026-10-19T14:59:40", now)).toBe("just now");
    expect(savedAgo("2026-10-19T14:48:00", now)).toBe("12 min ago");
    expect(savedAgo("2026-10-19T13:00:00", now)).toBe("2 h ago");
    expect(savedAgo("2026-10-18T09:00:00", now)).toBe("yesterday");
    expect(savedAgo("2026-10-14T09:00:00", now)).toBe("Oct 14");
    expect(savedOn("2026-10-14T09:00:00")).toBe("Oct 14");
    expect(savedAgo("nope", now)).toBe("");
  });
});

describe("shortSavedAgo", () => {
  const now = new Date("2026-10-28T12:00:00Z");
  const ago = (s: number) => new Date(now.getTime() - s * 1000).toISOString();
  it("says the age in one short unit", () => {
    expect(shortSavedAgo(ago(20), now)).toBe("now");
    expect(shortSavedAgo(ago(12 * 60), now)).toBe("12m");
    expect(shortSavedAgo(ago(2 * 3600 + 5), now)).toBe("2h");
    expect(shortSavedAgo(ago(4 * 86_400), now)).toBe("4d");
    expect(shortSavedAgo("not a date", now)).toBe("");
  });
});
