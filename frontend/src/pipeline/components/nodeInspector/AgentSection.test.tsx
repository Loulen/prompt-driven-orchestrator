import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import AgentSection from "./AgentSection";
import type { HarnessCatalog } from "../../../harness/model/catalog";
import { effortOffer, findHarnessOption } from "../../../harness/model/catalog";
import type { EditorNodeDef } from "../../model/pipeline";

vi.mock("../../../skill/components/SkillSelector", () => ({ default: () => null }));

const CATALOG: HarnessCatalog = {
  builtin: [
    {
      name: "claude",
      installed: true,
      models: ["sonnet", "opus"],
      modelContexts: {},
      efforts: ["low", "medium", "high"],
      modelEfforts: {},
      hasEffort: true,
      version: null,
    },
  ],
  descriptors: [],
};

function section(node: Partial<EditorNodeDef>, handleField = vi.fn()) {
  const option = findHarnessOption(CATALOG, "claude");
  return (
    <AgentSection
      node={{ id: "a", name: "a", type: "agent", outputs: [], ...node } as EditorNodeDef}
      handleField={handleField}
      harnessCatalog={CATALOG}
      agentProfiles={[]}
      skillBank={{ skills: [], folders: [], root_path: "" }}
      inheritedSkillTiers={[]}
      resolvedHarness="claude"
      harnessOption={option}
      effort={effortOffer(option, null)}
    />
  );
}

describe("AgentSection", () => {
  it("remembers a node's combination for that node only", () => {
    const handleField = vi.fn();
    const { rerender } = render(
      section({ id: "a", agent_choice: { mode: "custom", harness: "claude", model: "opus", effort: "high" } }, handleField),
    );
    rerender(section({ id: "a", agent_choice: { mode: "inherit" } }, handleField));
    rerender(section({ id: "b", agent_choice: { mode: "inherit" } }, handleField));
    fireEvent.click(screen.getByRole("radio", { name: "Custom" }));
    expect(handleField).toHaveBeenLastCalledWith("agent_choice", {
      mode: "custom",
      harness: "claude",
      model: "sonnet",
      effort: "medium",
    });
  });
});
