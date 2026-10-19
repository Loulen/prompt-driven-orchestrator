import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import NodeConfigView from "./NodeConfigView";
import { useEditStore } from "../store/editStore";
import { TooltipProvider } from "../../shared/ui/tooltip";

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));
vi.mock("../../settings/api/settings", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  fetchSettings: vi.fn().mockResolvedValue({ skills: [] }),
}));
vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: vi.fn().mockResolvedValue([]),
}));

function seedRunWithSkilledReviewer(type: "agent" | "script" = "agent", prompt = "Review this code.") {
  useEditStore.setState({
    openTabs: [
      {
        id: "__run__r1",
        scope: "run",
        runId: "r1",
        pipeline: {
          name: "p1",
          version: "1.0",
          variables: {},
          nodes: [
            {
              id: "rv1",
              name: "reviewer",
              type,
              interactive: true,
              inputs: [{ name: "in" }],
              outputs: [{ name: "out" }],
              view: { x: 0, y: 0 },
              skills: [{ id: "pdo-interactive", name: "pdo-interactive" }],
            },
          ],
          edges: [],
        },
        prompts: { rv1: prompt },
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "__run__r1",
    selection: { kind: "node", id: "rv1" },
  });
}

function renderView(archived = false) {
  return render(
    <TooltipProvider>
      <NodeConfigView archived={archived} />
    </TooltipProvider>,
  );
}

describe("NodeConfigView — the read-only Config of a run followed in « pilotage » (ADR-0085)", () => {
  it("shows the node's prompt and skills without any editable field", () => {
    seedRunWithSkilledReviewer();
    renderView();
    const view = screen.getByTestId("node-config-view");
    expect(screen.getByTestId("node-config-prompt")).toHaveTextContent("Review this code.");
    expect(screen.getByTestId("node-config-skill-list")).toHaveTextContent("pdo-interactive");
    expect(view).toHaveTextContent("/pdo-interactive");
    expect(view.querySelector("input, textarea")).toBeNull();
    expect(screen.queryByTestId("node-prompt-input")).toBeNull();
    expect(screen.queryByTestId("node-name-input")).toBeNull();
  });

  it("points to the Edit pencil › « Edit for this run » to change the node", () => {
    seedRunWithSkilledReviewer();
    renderView();
    expect(screen.getByTestId("node-config-hint")).toHaveTextContent("Edit for this run");
  });

  it("on an archived run, does not point to an Edit control that is not there", () => {
    seedRunWithSkilledReviewer();
    renderView(true);
    const hint = screen.getByTestId("node-config-hint");
    expect(hint).toHaveTextContent("This run is archived and read-only.");
    expect(hint).not.toHaveTextContent("Edit for this run");
  });

  it("shows a script node's bash, with no skills section", () => {
    seedRunWithSkilledReviewer("script", "echo hi");
    renderView();
    expect(screen.getByTestId("node-config-prompt")).toHaveTextContent("echo hi");
    expect(screen.getByTestId("node-config-view")).toHaveTextContent("Script (bash)");
    expect(screen.queryByTestId("node-config-skill-list")).toBeNull();
  });
});
