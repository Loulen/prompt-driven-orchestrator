import { beforeEach, describe, expect, it } from "vitest";
import { renderHook } from "@testing-library/react";
import { useCanvasLint } from "./useCanvasLint";
import { forgetSessionProfileNames, rememberProfileNames } from "../../settings/model/profileNames";
import type { OpenPipeline } from "./editState";

function tab(profileId: string): OpenPipeline {
  return {
    id: "flow",
    scope: "instance",
    pipeline: {
      name: "flow",
      version: "1.0",
      variables: {},
      nodes: [
        {
          id: "Qws9",
          name: "reviewer",
          type: "agent",
          inputs: [],
          outputs: [],
          agent_choice: { mode: "profile", profile_id: profileId },
        } as never,
      ],
      edges: [],
    },
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
  };
}

beforeEach(() => {
  localStorage.clear();
  forgetSessionProfileNames();
});

describe("the lint of a node whose agent profile is gone", () => {
  it("names the profile as the user knew it", () => {
    rememberProfileNames([{ id: "agp-20261109-f43", name: "Reviewer fast" }]);
    const { result } = renderHook(() => useCanvasLint(tab("agp-20261109-f43"), new Set(), new Set(), true));
    expect(result.current.visibleItems.map((item) => item.message)).toEqual([
      "Agent profile Reviewer fast no longer exists. Node reviewer falls back to the next tier. Pick a profile or set Custom.",
    ]);
  });

  it("says its id when this browser never saw it", () => {
    const { result } = renderHook(() => useCanvasLint(tab("agp-unseen"), new Set(), new Set(), true));
    expect(result.current.visibleItems[0].message).toMatch(/^Agent profile agp-unseen no longer exists/);
  });
});
