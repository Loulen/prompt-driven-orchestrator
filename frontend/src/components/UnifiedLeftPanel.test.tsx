import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import UnifiedLeftPanel from "./UnifiedLeftPanel";
import type { RunListEntry } from "../types";
import type { LibraryPipelineEntry } from "../api";
import { renameRun } from "../api";
import { useEditStore } from "../stores/editStore";

const mockRenameRun = vi.mocked(renameRun);

vi.mock("../api", () => ({
  cleanupRun: vi.fn().mockResolvedValue(undefined),
  forgetRun: vi.fn().mockResolvedValue(undefined),
  renameRun: vi.fn().mockResolvedValue(undefined),
  createPipeline: vi.fn().mockResolvedValue({ id: "new-pipe", scope: "repo", path: "/tmp" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  deleteTrigger: vi.fn().mockResolvedValue(undefined),
  fetchPipelines: vi.fn().mockResolvedValue([]),
}));

const noop = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    pipelines: [],
  });
});

function renderPanel({
  runs = [],
  libraryPipelines = [],
}: {
  runs?: RunListEntry[];
  libraryPipelines?: LibraryPipelineEntry[];
} = {}) {
  return render(
    <UnifiedLeftPanel
      runs={runs}
      selectedRunId={null}
      onSelectRun={noop}
      onNewRun={noop}
      libraryPipelines={libraryPipelines}
      onLibraryPipelinesChanged={noop}
    />,
  );
}

describe("UnifiedLeftPanel run display labels", () => {
  it("shows display label when run has a name", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-abc-123", pipeline_name: "review-loop", status: "running", started_at: null, name: "Fix auth bug" },
    ];
    renderPanel({ runs });

    expect(screen.getByTestId("run-display-label").textContent).toBe("Fix auth bug");
    expect(screen.getByTestId("run-pipeline-name").textContent).toBe("review-loop");
  });

  it("falls back to run-id when no name exists", () => {
    const runs: RunListEntry[] = [
      { run_id: "20260514-143000-abc1234", pipeline_name: "deploy-pipe", status: "completed", started_at: null },
    ];
    renderPanel({ runs });

    expect(screen.getByTestId("run-display-label").textContent).toBe("20260514-143000-abc1");
    expect(screen.getByTestId("run-pipeline-name").textContent).toBe("deploy-pipe");
  });

  it("falls back to run-id when name is null", () => {
    const runs: RunListEntry[] = [
      { run_id: "20260514-150000-def5678", pipeline_name: "my-pipe", status: "running", started_at: null, name: null },
    ];
    renderPanel({ runs });

    expect(screen.getByTestId("run-display-label").textContent).toBe("20260514-150000-def5");
  });

  it("shows two-line entries: label on top, pipeline name below", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-1", pipeline_name: "review-loop", status: "running", started_at: null, name: "Feature X" },
    ];
    renderPanel({ runs });

    const label = screen.getByTestId("run-display-label");
    const pipelineName = screen.getByTestId("run-pipeline-name");
    expect(label).toBeInTheDocument();
    expect(pipelineName).toBeInTheDocument();
    expect(label.textContent).toBe("Feature X");
    expect(pipelineName.textContent).toBe("review-loop");
  });

  it("renders edit icon for renaming", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-1", pipeline_name: "review-loop", status: "running", started_at: null, name: "My Run" },
    ];
    renderPanel({ runs });

    expect(screen.getByTestId("rename-button")).toBeInTheDocument();
  });

  it("shows empty state when no runs exist", () => {
    renderPanel();
    expect(screen.getByText("No runs yet")).toBeInTheDocument();
  });

  it("shows rename input when edit icon is clicked", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-1", pipeline_name: "pipe", status: "running", started_at: null, name: "Old Name" },
    ];
    renderPanel({ runs });

    fireEvent.click(screen.getByTestId("rename-button"));

    const input = screen.getByTestId("rename-input") as HTMLInputElement;
    expect(input).toBeInTheDocument();
    expect(input.value).toBe("Old Name");
  });

  it("calls renameRun on Enter", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-enter", pipeline_name: "pipe", status: "completed", started_at: null, name: "Before" },
    ];
    renderPanel({ runs });

    fireEvent.click(screen.getByTestId("rename-button"));
    const input = screen.getByTestId("rename-input");
    fireEvent.change(input, { target: { value: "After" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(mockRenameRun).toHaveBeenCalledWith("run-enter", "After");
  });

  it("cancels rename on Escape without calling API", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-esc", pipeline_name: "pipe", status: "running", started_at: null, name: "Keep" },
    ];
    renderPanel({ runs });

    fireEvent.click(screen.getByTestId("rename-button"));
    fireEvent.keyDown(screen.getByTestId("rename-input"), { key: "Escape" });

    expect(mockRenameRun).not.toHaveBeenCalled();
    expect(screen.queryByTestId("rename-input")).not.toBeInTheDocument();
    expect(screen.getByTestId("run-display-label").textContent).toBe("Keep");
  });
});

describe("UnifiedLeftPanel three-tab strip", () => {
  it("renders Runs, Triggers and Library tabs", () => {
    renderPanel();
    expect(screen.getByRole("tab", { name: "Runs" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Triggers" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Library" })).toBeInTheDocument();
  });

  it("defaults to the Runs tab", () => {
    renderPanel();
    expect(screen.getByText("No runs yet")).toBeInTheDocument();
  });

  it("switches to the Triggers tab and shows its empty state", () => {
    renderPanel();
    fireEvent.click(screen.getByRole("tab", { name: "Triggers" }));
    expect(screen.getByText(/no triggers yet/i)).toBeInTheDocument();
  });

  it("shows a provenance badge on a triggered run row", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-trig", pipeline_name: "auditor", status: "running", started_at: null, triggered_by: "trg-1" },
    ];
    renderPanel({ runs });
    expect(screen.getByTestId("run-trigger-badge")).toBeInTheDocument();
  });

  it("does not show a provenance badge on a manual run row", () => {
    const runs: RunListEntry[] = [
      { run_id: "run-manual", pipeline_name: "auditor", status: "running", started_at: null },
    ];
    renderPanel({ runs });
    expect(screen.queryByTestId("run-trigger-badge")).not.toBeInTheDocument();
  });

  it("renders trigger rows on the Triggers tab", () => {
    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={[]}
        onLibraryPipelinesChanged={noop}
        triggers={[
          {
            id: "trg-1",
            name: "Nightly audit",
            pipeline_id: "auditor",
            pipeline_name: "Auditor",
            input_template: "",
            variables: {},
            cron: "0 9 * * *",
            overlap_policy: "skip",
            enabled: true,
          },
        ]}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Triggers" }));
    expect(screen.getByText("Nightly audit")).toBeInTheDocument();
  });
});
