import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import RunsListPanel from "./RunsListPanel";
import type { LibraryEntry, LibraryPipelineEntry } from "../api";
import type { RunListEntry } from "../types";

vi.mock("../api", () => ({
  cleanupRun: vi.fn().mockResolvedValue(undefined),
  forgetRun: vi.fn().mockResolvedValue(undefined),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
}));

const noop = () => {};

function renderPanel({
  runs = [],
  libraryPipelines = [],
  libraryNodes = [],
}: {
  runs?: RunListEntry[];
  libraryPipelines?: LibraryPipelineEntry[];
  libraryNodes?: LibraryEntry[];
} = {}) {
  return render(
    <RunsListPanel
      runs={runs}
      selectedRunId={null}
      onSelectRun={noop}
      onNewRun={noop}
      libraryPipelines={libraryPipelines}
      libraryNodes={libraryNodes}
      onLibraryPipelinesChanged={noop}
    />,
  );
}

describe("RunsListPanel Library section", () => {
  it("renders the Library header", () => {
    renderPanel();
    expect(screen.getByText("Library")).toBeInTheDocument();
  });

  it("shows pipeline templates sub-section", () => {
    renderPanel();
    expect(screen.getByText("Pipeline templates")).toBeInTheDocument();
  });

  it("shows reusable nodes sub-section", () => {
    renderPanel();
    expect(screen.getByText("Reusable nodes")).toBeInTheDocument();
  });

  it("shows empty state when no library entries exist", () => {
    renderPanel();
    expect(screen.getByText("No starred templates yet")).toBeInTheDocument();
    expect(screen.getByText("No saved nodes yet")).toBeInTheDocument();
  });

  it("renders starred pipeline template entries", () => {
    const pipelines: LibraryPipelineEntry[] = [
      { id: "review-pipeline", name: "Review Pipeline", node_count: 5, modified: null, yaml: "" },
      { id: "deploy-pipeline", name: "Deploy Pipeline", node_count: 3, modified: null, yaml: "" },
    ];
    renderPanel({ libraryPipelines: pipelines });

    expect(screen.getByText("Review Pipeline")).toBeInTheDocument();
    expect(screen.getByText("Deploy Pipeline")).toBeInTheDocument();
    expect(screen.getByText("5n")).toBeInTheDocument();
    expect(screen.getByText("3n")).toBeInTheDocument();
  });

  it("renders reusable node entries", () => {
    const nodes: LibraryEntry[] = [
      {
        name: "Reviewer",
        type: "doc-only",
        inputs: [],
        outputs: [],
        interactive: false,
        prompt: "review",
      },
    ];
    renderPanel({ libraryNodes: nodes });

    expect(screen.getByText("Reviewer")).toBeInTheDocument();
    expect(screen.getByText("doc-only")).toBeInTheDocument();
  });
});
