import { describe, expect, it } from "vitest";
import { INITIAL_DRAFT_STATE } from "./draftSession";
import { hasUnsavedWork, type OpenPipeline } from "./editState";

function tab(patch: Partial<OpenPipeline> = {}): OpenPipeline {
  return {
    id: "flow",
    scope: "instance",
    pipeline: { name: "flow", variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
    dirty: true,
    externalDirty: false,
    ...patch,
  } as OpenPipeline;
}

describe("hasUnsavedWork", () => {
  it("counts a changed tab and a failed save", () => {
    expect(hasUnsavedWork(tab())).toBe(true);
    expect(hasUnsavedWork(tab({ dirty: false, saveError: { message: "disk full" } }))).toBe(true);
    expect(hasUnsavedWork(tab({ dirty: false }))).toBe(false);
  });

  it("leaves out a live shared draft: its changes outlive the tab (ADR-0088)", () => {
    const live = { ...INITIAL_DRAFT_STATE, status: "live" as const, synced: true, canWrite: true, changes: 1 };
    expect(hasUnsavedWork(tab({ draft: live }))).toBe(false);
  });

  it("keeps a draft whose edits are still local (offline)", () => {
    const offline = { ...INITIAL_DRAFT_STATE, status: "offline" as const, synced: true, canWrite: true, changes: 1 };
    expect(hasUnsavedWork(tab({ draft: offline }))).toBe(true);
  });
});
