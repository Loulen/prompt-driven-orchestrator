import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import UnifiedLeftPanel from "./UnifiedLeftPanel";
import type { PipelineListEntry, RunListEntry } from "../types";
import type { LibraryPipelineEntry } from "../api";
import { deleteLibraryPipeline, deletePipeline, duplicateLibraryPipeline, fetchPipelines, renameRun } from "../api";
import { useEditStore } from "../stores/editStore";

const mockRenameRun = vi.mocked(renameRun);
const mockDeletePipeline = vi.mocked(deletePipeline);
const mockFetchPipelines = vi.mocked(fetchPipelines);
const mockDuplicateLibraryPipeline = vi.mocked(duplicateLibraryPipeline);
const mockDeleteLibraryPipeline = vi.mocked(deleteLibraryPipeline);

vi.mock("../api", () => ({
  cleanupRun: vi.fn().mockResolvedValue(undefined),
  forgetRun: vi.fn().mockResolvedValue(undefined),
  renameRun: vi.fn().mockResolvedValue(undefined),
  createPipeline: vi.fn().mockResolvedValue({ id: "new-pipe", scope: "repo", path: "/tmp" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  duplicateLibraryPipeline: vi
    .fn()
    .mockResolvedValue({ id: "x-copy", scope: "user", entry: null }),
  deletePipeline: vi.fn().mockResolvedValue(undefined),
  deleteTrigger: vi.fn().mockResolvedValue(undefined),
  fetchPipelines: vi.fn().mockResolvedValue([]),
  fetchPipeline: vi.fn().mockResolvedValue({
    scope: "library",
    pipeline: { name: "simple-bugfix", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
  }),
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

// #258 — the Runs list groups by project (target repo), conditionally: only when
// the on-screen runs span ≥ 2 distinct repos. Single-repo stays flat (no header,
// no per-row repo badge). The Runs tab is the default tab, so runs render at once.
describe("UnifiedLeftPanel runs grouped by repo (#258)", () => {
  it("stays flat (no repo-group header) when all runs share one repo", () => {
    const runs: RunListEntry[] = [
      { run_id: "r1", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/foo" },
      { run_id: "r2", pipeline_name: "p", status: "completed", started_at: null, effective_repo: "/repos/foo" },
    ];
    renderPanel({ runs });
    expect(screen.queryByTestId("run-repo-group")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("run-display-label")).toHaveLength(2);
  });

  it("renders one repo-group header per distinct repo, alphabetical, when ≥ 2 repos", () => {
    const runs: RunListEntry[] = [
      { run_id: "r1", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/zebra" },
      { run_id: "r2", pipeline_name: "p", status: "completed", started_at: null, effective_repo: "/repos/alpha" },
      { run_id: "r3", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/zebra" },
    ];
    renderPanel({ runs });
    expect(screen.getAllByTestId("run-repo-group")).toHaveLength(2);
    const labels = screen.getAllByTestId("run-repo-label").map((el) => el.textContent);
    expect(labels).toEqual(["alpha", "zebra"]);
    // The full path is available on the header for hover.
    const alphaGroup = screen.getAllByTestId("run-repo-group")[0];
    expect(within(alphaGroup).getByText("alpha").closest("div")).toHaveAttribute(
      "title",
      "/repos/alpha",
    );
  });

  it("counts archived rows toward the threshold (a 2nd-repo archived run flips to grouped)", () => {
    const runs: RunListEntry[] = [
      { run_id: "r1", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/foo" },
      { run_id: "r2", pipeline_name: "p", status: "archived", started_at: null, effective_repo: "/repos/bar" },
    ];
    renderPanel({ runs });
    expect(screen.getAllByTestId("run-repo-group")).toHaveLength(2);
  });

  it("groups a null-target run (effective_repo resolved server-side) with no catch-all bucket", () => {
    const runs: RunListEntry[] = [
      { run_id: "r1", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/alpha" },
      { run_id: "r2", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/root" },
    ];
    renderPanel({ runs });
    const labels = screen.getAllByTestId("run-repo-label").map((el) => el.textContent);
    // Exactly two groups, both resolved paths — a phantom catch-all bucket would
    // add a third label.
    expect(labels).toEqual(["alpha", "root"]);
    expect(screen.getAllByTestId("run-repo-group")).toHaveLength(2);
  });
});

// #216 — A `scope: "library"` entry surfaced in the merged /pipelines list must
// delete via the library store. The pre-fix code called removePipeline(id) with
// no scope, which routed to DELETE /pipelines/{id} and destroyed the same-named
// repo YAML + .prompts/ sidecar.
describe("UnifiedLeftPanel library-scoped delete (#216)", () => {
  const libEntry: PipelineListEntry = {
    id: "simple-bugfix",
    name: "simple-bugfix",
    scope: "library",
    path: "/home/u/.pdo/library/pipelines/simple-bugfix.yaml",
    node_count: 3,
    modified: null,
    variables: {},
  };

  it("forwards scope=library to deletePipeline instead of the repo path", async () => {
    mockFetchPipelines.mockResolvedValueOnce([libEntry]);

    renderPanel();
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));

    // Row renders once loadPipelines() resolves.
    await screen.findByText("simple-bugfix");

    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(mockDeletePipeline).toHaveBeenCalledWith("simple-bugfix", "library"),
    );
  });
});

// #224 — a hover Copy icon on library-only rows duplicates the template into an
// unlinked clone. It must NOT appear on starred block-1 rows (which carry a
// working pipeline id, not a library id).
describe("UnifiedLeftPanel library duplicate (#224)", () => {
  const libOnly: LibraryPipelineEntry = {
    id: "fixture",
    name: "fixture",
    scope: "user",
    node_count: 3,
    modified: null,
    yaml: "name: fixture\n",
    pipeline: { name: "fixture", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
  };

  function renderWithLib(onChanged: () => void = noop) {
    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={[libOnly]}
        onLibraryPipelinesChanged={onChanged}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));
  }

  it("renders the duplicate button on a library-only row", () => {
    renderWithLib();
    expect(screen.getByTestId("library-only-entry")).toBeInTheDocument();
    expect(screen.getByTestId("library-duplicate-button")).toBeInTheDocument();
  });

  it("calls duplicateLibraryPipeline(id) and refreshes on click", async () => {
    const onChanged = vi.fn();
    renderWithLib(onChanged);

    fireEvent.click(screen.getByTestId("library-duplicate-button"));

    await waitFor(() =>
      expect(mockDuplicateLibraryPipeline).toHaveBeenCalledWith("fixture"),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("busy-guards a double-click so it fires once", async () => {
    renderWithLib();
    const btn = screen.getByTestId("library-duplicate-button");
    fireEvent.click(btn);
    fireEvent.click(btn);

    await waitFor(() =>
      expect(mockDuplicateLibraryPipeline).toHaveBeenCalledTimes(1),
    );
  });

  it("does not render a duplicate button on a starred block-1 working row", async () => {
    // A working pipeline whose name matches a library entry renders in block 1
    // (starred) and filters the library-only row out. It exposes Delete, never
    // a duplicate affordance.
    mockFetchPipelines.mockResolvedValueOnce([
      {
        id: "fixture",
        name: "fixture",
        scope: "repo",
        path: "/repo/.pdo/pipelines/fixture.yaml",
        node_count: 3,
        modified: null,
        variables: {},
      },
    ]);

    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={[libOnly]}
        onLibraryPipelinesChanged={noop}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));

    await screen.findByTestId("left-panel-star");
    expect(screen.getByRole("button", { name: "Delete pipeline" })).toBeInTheDocument();
    expect(screen.queryByTestId("library-duplicate-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("library-only-entry")).not.toBeInTheDocument();
  });

  // #273 — regression: once /pipelines began merging library-scope entries
  // (#216), a user-scoped library pipeline appears in BOTH lists with the same
  // name. The block-2 name-absence filter then drops it (its name matches a
  // /pipelines row), so the only Copy button used to vanish. Block 1 must now
  // carry its own Copy on scope:"library" rows.
  it("keeps the Copy button reachable when a scope:'library' row also sits in /pipelines (#273)", async () => {
    // The regression's exact condition: same NAME in BOTH lists.
    mockFetchPipelines.mockResolvedValueOnce([
      {
        id: "fixture",
        name: "fixture", // == libOnly.name
        scope: "library", // the regression's scope
        path: "/home/u/.pdo/library/pipelines/fixture.yaml",
        node_count: 3,
        modified: null,
        variables: {},
      } satisfies PipelineListEntry,
    ]);
    renderWithLib(); // libraryPipelines={[libOnly]}, opens Library tab
    await screen.findByText("fixture"); // block-1 scope:library row mounts
    // DOM-PRESENCE, not hover-visual: jsdom does not apply Tailwind group-hover.
    // libOnly is filtered out of block 2 (name match) ⇒ exactly one button.
    expect(screen.getByTestId("library-duplicate-button")).toBeInTheDocument();
    expect(screen.queryByTestId("library-only-entry")).not.toBeInTheDocument();
  });

  it("the #273 block-1 Copy calls duplicateLibraryPipeline(id) and refreshes", async () => {
    const onChanged = vi.fn();
    mockFetchPipelines.mockResolvedValueOnce([
      {
        id: "fixture",
        name: "fixture",
        scope: "library",
        path: "/home/u/.pdo/library/pipelines/fixture.yaml",
        node_count: 3,
        modified: null,
        variables: {},
      } satisfies PipelineListEntry,
    ]);
    renderWithLib(onChanged);
    await screen.findByText("fixture");

    fireEvent.click(screen.getByTestId("library-duplicate-button"));

    // p.id is the HOME library file-stem — the same id the endpoint resolves.
    await waitFor(() =>
      expect(mockDuplicateLibraryPipeline).toHaveBeenCalledWith("fixture"),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });
});

// #227 — Deleting a starred pipeline must be able to cascade-remove its durable
// Library copy. The copy's id is an independently derived slug (it can diverge
// from the working pipeline's id), so the twin is matched on NAME. The cascade
// is opt-in (checkbox default OFF) and only offered on a unique same-name twin.
describe("UnifiedLeftPanel delete cascades to library copy (#227)", () => {
  // A library twin whose id deliberately differs from the working pipeline's id
  // — proves the cascade deletes by the twin's id, found via the name match.
  const twin: LibraryPipelineEntry = {
    id: "fixture-lib-slug",
    name: "fixture",
    scope: "user",
    node_count: 3,
    modified: null,
    yaml: "name: fixture\n",
    pipeline: { name: "fixture", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
  };

  const workingRow: PipelineListEntry = {
    id: "fixture-repo-id",
    name: "fixture",
    scope: "repo",
    path: "/repo/.pdo/pipelines/fixture.yaml",
    node_count: 3,
    modified: null,
    variables: {},
  };

  function renderStarredRow(libraryPipelines: LibraryPipelineEntry[]) {
    // Make the working-pipeline list deterministic regardless of any leftover
    // `mockResolvedValueOnce` from earlier tests (vitest's clearAllMocks does
    // not drain the once-queue): set the base resolved value here.
    mockFetchPipelines.mockResolvedValue([workingRow]);
    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={libraryPipelines}
        onLibraryPipelinesChanged={noop}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));
    // Wait on the row NAME (always rendered) — the star only shows when a twin
    // exists, so the no-twin case must not block on it.
    return screen.findByText(workingRow.name);
  }

  it("shows the cascade checkbox when the row has exactly one same-name copy", async () => {
    await renderStarredRow([twin]);
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));

    const box = screen.getByTestId("delete-cascade-checkbox");
    expect(box).toBeInTheDocument();
    expect(screen.getByText("Also remove the Library copy")).toBeInTheDocument();
  });

  it("hides the cascade checkbox when no same-name copy exists", async () => {
    await renderStarredRow([]);
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));

    expect(screen.getByTestId("confirm-delete-backdrop")).toBeInTheDocument();
    expect(screen.queryByTestId("delete-cascade-checkbox")).not.toBeInTheDocument();
  });

  it("defaults the cascade checkbox to OFF", async () => {
    await renderStarredRow([twin]);
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));

    const box = screen.getByTestId("delete-cascade-checkbox") as HTMLInputElement;
    expect(box.checked).toBe(false);
  });

  it("resets the checkbox to OFF when reopened on a different target", async () => {
    // Two starred working rows, each with a unique same-name twin.
    const alpha: PipelineListEntry = { ...workingRow, id: "alpha-id", name: "alpha", path: "/repo/.pdo/pipelines/alpha.yaml" };
    const beta: PipelineListEntry = { ...workingRow, id: "beta-id", name: "beta", path: "/repo/.pdo/pipelines/beta.yaml" };
    mockFetchPipelines.mockResolvedValue([alpha, beta]);
    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={[
          { ...twin, id: "alpha-twin", name: "alpha" },
          { ...twin, id: "beta-twin", name: "beta" },
        ]}
        onLibraryPipelinesChanged={noop}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));
    await screen.findByText("alpha");

    const trashButtons = screen.getAllByRole("button", { name: "Delete pipeline" });
    // Open on alpha (index 0), tick the box, then cancel.
    fireEvent.click(trashButtons[0]);
    fireEvent.click(screen.getByTestId("delete-cascade-checkbox"));
    expect((screen.getByTestId("delete-cascade-checkbox") as HTMLInputElement).checked).toBe(true);
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    // Reopen on beta (index 1) — the checkbox must be back to OFF.
    fireEvent.click(screen.getAllByRole("button", { name: "Delete pipeline" })[1]);
    expect((screen.getByTestId("delete-cascade-checkbox") as HTMLInputElement).checked).toBe(false);
  });

  it("cascades deleteLibraryPipeline(twin.id) when the box is ticked", async () => {
    await renderStarredRow([twin]);
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));
    fireEvent.click(screen.getByTestId("delete-cascade-checkbox"));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(mockDeletePipeline).toHaveBeenCalledWith("fixture-repo-id", "repo"),
    );
    await waitFor(() =>
      // Deletes the twin by its (divergent) library id, found via the name match.
      expect(mockDeleteLibraryPipeline).toHaveBeenCalledWith("fixture-lib-slug"),
    );
  });

  it("does NOT cascade when the box is left unticked", async () => {
    await renderStarredRow([twin]);
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(mockDeletePipeline).toHaveBeenCalledWith("fixture-repo-id", "repo"),
    );
    expect(mockDeleteLibraryPipeline).not.toHaveBeenCalled();
  });

  it("suppresses the checkbox on an ambiguous double-star (2+ same-name copies)", async () => {
    await renderStarredRow([
      { ...twin, id: "fixture-repo-copy", scope: "repo" },
      { ...twin, id: "fixture-user-copy", scope: "user" },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));

    expect(screen.getByTestId("confirm-delete-backdrop")).toBeInTheDocument();
    expect(screen.queryByTestId("delete-cascade-checkbox")).not.toBeInTheDocument();
  });

  it("leaves the block-2 library-only delete unaffected (direct, no modal)", async () => {
    // No matching /pipelines entry ⇒ the twin renders as a library-only row,
    // whose own trash deletes the copy directly with no confirm modal (#227 d).
    mockFetchPipelines.mockResolvedValue([]);
    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={[twin]}
        onLibraryPipelinesChanged={noop}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));
    await screen.findByTestId("library-only-entry");

    fireEvent.click(screen.getByRole("button", { name: "Remove from library" }));

    await waitFor(() =>
      expect(mockDeleteLibraryPipeline).toHaveBeenCalledWith("fixture-lib-slug"),
    );
    expect(screen.queryByTestId("confirm-delete-backdrop")).not.toBeInTheDocument();
    expect(mockDeletePipeline).not.toHaveBeenCalled();
  });
});
