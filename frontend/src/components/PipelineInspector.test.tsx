import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PipelineInspector from "./PipelineInspector";
import { useEditStore } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

vi.mock("../api", () => ({
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "my-pipeline", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));

function seedTab(libraryBinding?: { id: string | null; scope: "repo" | "user" | null }) {
  useEditStore.setState({
    openTabs: [
      {
        id: "p1",
        scope: "repo",
        pipeline: {
          name: "My Pipeline",
          version: "1.0",
          variables: {},
          nodes: [
            {
              id: "start",
              name: "Start",
              type: "start",
              interactive: false,
              inputs: [],
              outputs: [{ name: "user_prompt", repeated: false, side: "right" }],
            },
            {
              id: "end",
              name: "End",
              type: "end",
              interactive: false,
              inputs: [{ name: "result", repeated: false, side: "left" }],
              outputs: [],
            },
          ],
          edges: [
            {
              source: { node: "start", port: "user_prompt" },
              target: { node: "end", port: "result" },
            },
          ],
        },
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
        libraryId: libraryBinding?.id ?? null,
        libraryScope: libraryBinding?.scope ?? null,
      },
    ],
    activeTabId: "p1",
    selection: { kind: "none", id: null },
  });
}

function renderInspector() {
  return render(
    <TooltipProvider>
      <PipelineInspector />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
});

describe("PipelineInspector", () => {
  // Inline star buttons were removed from PipelineInspector — the canvas-level
  // PipelineStar is now the single source of truth (see PipelineStar.tsx).
  it("renders identity and does not show any inline star", () => {
    seedTab();
    renderInspector();

    expect(screen.getByText("Pipeline Inspector")).toBeInTheDocument();
    expect(screen.queryByTitle("Star as template")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Remove from library")).not.toBeInTheDocument();
  });

  it("does not show an inline star even when the pipeline is in the library", () => {
    seedTab({ id: "my-pipeline", scope: "repo" });
    renderInspector();

    expect(screen.queryByTitle("Star as template")).not.toBeInTheDocument();
    expect(screen.queryByTitle("Remove from library")).not.toBeInTheDocument();
  });

  it("does not expose legacy pipeline scope controls", () => {
    seedTab();
    const { rerender } = renderInspector();
    expect(screen.queryByTestId("pipeline-inspector-scope")).not.toBeInTheDocument();

    seedTab({ id: "my-pipeline", scope: "repo" });
    rerender(
      <TooltipProvider>
        <PipelineInspector />
      </TooltipProvider>,
    );
    expect(screen.queryByTestId("pipeline-inspector-scope")).not.toBeInTheDocument();
  });

  // Prompt-required checkbox (#158)
  it("checks 'Prompt required' by default when the flag is absent", () => {
    seedTab();
    renderInspector();
    const checkbox = screen.getByTestId("prompt-required-checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(true);
  });

  it("unchecks 'Prompt required' when the pipeline is prompt-optional", () => {
    seedTab();
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === "p1" ? { ...t, pipeline: { ...t.pipeline, prompt_required: false } } : t,
      ),
    }));
    renderInspector();
    const checkbox = screen.getByTestId("prompt-required-checkbox") as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
  });

  it("toggling the checkbox persists prompt_required to the store", () => {
    seedTab();
    renderInspector();
    const checkbox = screen.getByTestId("prompt-required-checkbox");

    fireEvent.click(checkbox);
    expect(
      useEditStore.getState().openTabs[0].pipeline.prompt_required,
    ).toBe(false);

    fireEvent.click(checkbox);
    expect(
      useEditStore.getState().openTabs[0].pipeline.prompt_required,
    ).toBe(true);
  });

  // Pipeline-wide diagnostics live on the canvas overlay (EditCanvas), which is the
  // single source of truth — the same consolidation as the inline-star removal above.
  // The inspector must NOT render its own copy, else the banner shows twice when no
  // node is selected (#63).
  it("does not render the lint banner — pipeline diagnostics live on the canvas overlay, not the inspector (#63)", () => {
    seedTab();
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) =>
        t.id === "p1"
          ? { ...t, diagnostics: ["node 'reviewer' receives edges from 2 isolated nodes without a Merge"] }
          : t,
      ),
    }));
    renderInspector();
    expect(screen.getByText("Pipeline Inspector")).toBeInTheDocument(); // inspector did mount
    expect(screen.queryByTestId("lint-banner")).not.toBeInTheDocument(); // but no banner
  });
});
