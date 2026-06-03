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

describe("RunsListPanel run status rendering", () => {
  it("renders paused run with correct status dot", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-1", pipeline_name: "test-pipe", status: "paused", started_at: null },
    ];
    renderPanel({ runs });
    const dot = document.querySelector(".bg-st-paused");
    expect(dot).toBeInTheDocument();
  });

  it("renders all run statuses without errors", () => {
    const statuses: RunListEntry["status"][] = [
      "running", "awaiting_user", "completed", "failed", "halted", "paused", "archived",
    ];
    const runs: RunListEntry[] = statuses.map((status, i) => ({
      run_id: `run-${i}`,
      pipeline_name: `pipe-${status}`,
      status,
      started_at: null,
    }));
    renderPanel({ runs });
    for (const status of statuses) {
      expect(screen.getByText(`pipe-${status}`)).toBeInTheDocument();
    }
  });
});

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
    const emptyDef = (name: string) => ({ name, variables: {}, nodes: [], edges: [] });
    const pipelines: LibraryPipelineEntry[] = [
      { id: "review-pipeline", name: "Review Pipeline", scope: "repo", node_count: 5, modified: null, yaml: "", pipeline: emptyDef("Review Pipeline"), prompts: {} },
      { id: "deploy-pipeline", name: "Deploy Pipeline", scope: "repo", node_count: 3, modified: null, yaml: "", pipeline: emptyDef("Deploy Pipeline"), prompts: {} },
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
