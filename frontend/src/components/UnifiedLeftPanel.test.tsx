import { useState } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import UnifiedLeftPanel from "./UnifiedLeftPanel";
import type { PipelineListEntry, RunListEntry, Trigger } from "../types";
import type { LibraryPipelineEntry } from "../api";
import { deleteLibraryPipeline, deletePipeline, duplicateLibraryPipeline, fetchPipelines, openRunShell, pauseRun, renameRun, resumeRun, retryAll } from "../api";
import { useEditStore } from "../stores/editStore";

const mockRenameRun = vi.mocked(renameRun);
const mockDeletePipeline = vi.mocked(deletePipeline);
const mockFetchPipelines = vi.mocked(fetchPipelines);
const mockDuplicateLibraryPipeline = vi.mocked(duplicateLibraryPipeline);
const mockDeleteLibraryPipeline = vi.mocked(deleteLibraryPipeline);
const mockOpenRunShell = vi.mocked(openRunShell);
const mockPauseRun = vi.mocked(pauseRun);
const mockResumeRun = vi.mocked(resumeRun);
const mockRetryAll = vi.mocked(retryAll);

vi.mock("../api", () => ({
  cleanupRun: vi.fn().mockResolvedValue(undefined),
  forgetRun: vi.fn().mockResolvedValue(undefined),
  pauseRun: vi.fn().mockResolvedValue(undefined),
  resumeRun: vi.fn().mockResolvedValue(undefined),
  retryAll: vi.fn().mockResolvedValue({ run_id: "offspring-1" }),
  renameRun: vi.fn().mockResolvedValue(undefined),
  createPipeline: vi.fn().mockResolvedValue({ id: "new-pipe", scope: "repo", path: "/tmp" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  duplicateLibraryPipeline: vi
    .fn()
    .mockResolvedValue({ id: "x-copy", scope: "user", entry: null }),
  deletePipeline: vi.fn().mockResolvedValue(undefined),
  deleteTrigger: vi.fn().mockResolvedValue(undefined),
  openRunShell: vi
    .fn()
    .mockResolvedValue({ session: "pdo-shell-run-term-1", created: true }),
  fetchPipelines: vi.fn().mockResolvedValue([]),
  fetchPipeline: vi.fn().mockResolvedValue({
    scope: "library",
    pipeline: { name: "simple-bugfix", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
  }),
}));

// Stub the shell modal so a click that mounts it doesn't drag in xterm.js / a
// real PTY WebSocket. It echoes the `session` prop for assertions (#316).
vi.mock("./RunShellModal", () => ({
  default: ({ session }: { session: string }) => (
    <div data-testid="run-shell-modal" data-session={session} />
  ),
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
  selectedRunId = null,
  triggers,
}: {
  runs?: RunListEntry[];
  libraryPipelines?: LibraryPipelineEntry[];
  selectedRunId?: string | null;
  triggers?: Trigger[];
} = {}) {
  return render(
    <UnifiedLeftPanel
      runs={runs}
      selectedRunId={selectedRunId}
      onSelectRun={noop}
      onNewRun={noop}
      libraryPipelines={libraryPipelines}
      onLibraryPipelinesChanged={noop}
      triggers={triggers}
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

  it("excludes archived rows from the repo-group threshold and lists them in the Archived section", () => {
    const runs: RunListEntry[] = [
      { run_id: "r1", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/foo" },
      { run_id: "r2", pipeline_name: "p", status: "archived", started_at: null, effective_repo: "/repos/bar" },
    ];
    renderPanel({ runs });
    // Only one *active* repo ⇒ active list stays flat (no repo-group header).
    expect(screen.queryByTestId("run-repo-group")).not.toBeInTheDocument();
    // The archived run is pulled into the Archived section instead.
    expect(screen.getByTestId("run-archived-section")).toBeInTheDocument();
    expect(screen.getByTestId("run-archived-count").textContent).toBe("(1)");
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

// #136 — archived runs are lifted out of the active list into their own flat,
// collapsible "Archived" section below it. Collapsed by default; auto-opens when
// the currently-selected run is (or becomes) archived, but stays collapsible.
describe("UnifiedLeftPanel archived section (#136)", () => {
  const active: RunListEntry = {
    run_id: "run-active",
    pipeline_name: "p",
    status: "running",
    started_at: null,
    name: "Active One",
  };
  const archived: RunListEntry = {
    run_id: "run-archived",
    pipeline_name: "p",
    status: "archived",
    started_at: null,
    name: "Archived One",
  };

  it("collapses the Archived section by default (toggle shown, body hidden)", () => {
    renderPanel({ runs: [active, archived] });
    // Toggle + count are always visible…
    expect(screen.getByTestId("run-archived-toggle")).toBeInTheDocument();
    expect(screen.getByTestId("run-archived-count").textContent).toBe("(1)");
    // …but the archived row's body stays folded: only the active row renders.
    expect(screen.getAllByTestId("run-display-label")).toHaveLength(1);
    expect(screen.queryByText("Archived One")).not.toBeInTheDocument();
  });

  it("expands and re-collapses when the toggle is clicked", () => {
    renderPanel({ runs: [active, archived] });
    fireEvent.click(screen.getByTestId("run-archived-toggle"));
    expect(screen.getByText("Archived One")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("run-archived-toggle"));
    expect(screen.queryByText("Archived One")).not.toBeInTheDocument();
  });

  it("auto-expands when the selected run is already archived on mount", () => {
    renderPanel({ runs: [active, archived], selectedRunId: "run-archived" });
    // Visible without any click — the mount-time initializer path.
    expect(screen.getByText("Archived One")).toBeInTheDocument();
  });

  it("auto-expands when the selected run is archived mid-session", () => {
    const props = {
      selectedRunId: "run-active",
      onSelectRun: noop,
      onNewRun: noop,
      libraryPipelines: [],
      onLibraryPipelinesChanged: noop,
    };
    const { rerender } = render(
      <UnifiedLeftPanel runs={[active, archived]} {...props} />,
    );
    // Section collapsed to start (selected run is active): old archived row hidden.
    expect(screen.queryByText("Archived One")).not.toBeInTheDocument();

    // The selected run flips to archived (its worktree got reaped mid-session).
    const nowArchived: RunListEntry = { ...active, status: "archived" };
    rerender(<UnifiedLeftPanel runs={[nowArchived, archived]} {...props} />);

    // The transition force-opens the section — the previously-hidden row appears.
    expect(screen.getByText("Archived One")).toBeInTheDocument();
  });

  it("stays collapsible while a selected archived run is present (anti-dead-lock, decision 4)", () => {
    // Auto-open on mount because the selected run is archived…
    renderPanel({ runs: [archived], selectedRunId: "run-archived" });
    expect(screen.getByText("Archived One")).toBeInTheDocument();
    // …then a single click must still collapse it. A naive
    // `open = archivedOpen || some(selected)` gate would pin it open forever.
    fireEvent.click(screen.getByTestId("run-archived-toggle"));
    expect(screen.queryByText("Archived One")).not.toBeInTheDocument();
  });

  it("renders no Archived section when there are no archived runs", () => {
    renderPanel({ runs: [active] });
    expect(screen.queryByTestId("run-archived-section")).not.toBeInTheDocument();
  });

  it("does not show 'No runs yet' for an archived-only list", () => {
    renderPanel({ runs: [archived] });
    expect(screen.queryByText("No runs yet")).not.toBeInTheDocument();
    expect(screen.getByTestId("run-archived-section")).toBeInTheDocument();
  });

  it("groups the active list by repo while excluding archived runs from the threshold", () => {
    const runs: RunListEntry[] = [
      { run_id: "a1", pipeline_name: "p", status: "running", started_at: null, effective_repo: "/repos/alpha" },
      { run_id: "a2", pipeline_name: "p", status: "completed", started_at: null, effective_repo: "/repos/zebra" },
      { run_id: "a3", pipeline_name: "p", status: "archived", started_at: null, effective_repo: "/repos/gamma" },
    ];
    renderPanel({ runs });
    // Two active repos ⇒ two groups; the archived third repo adds no phantom group.
    expect(screen.getAllByTestId("run-repo-group")).toHaveLength(2);
    expect(screen.getByTestId("run-archived-count").textContent).toBe("(1)");
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

// #371 — after Duplicate, the new copy row must be immediately usable (proper
// button, "library" badge, opens on click) WITHOUT a full page reload. The bug:
// the duplicate handler refreshed only /library/pipelines, so the copy landed
// there tagged with its raw storage scope "user" and — being absent from
// /pipelines — fell through to the degraded block-2 <div> (wrong "user" badge,
// no button role, dead click). A reload re-fetched /pipelines (where the daemon
// tags the copy scope:"library"), which repaired all three symptoms. The fix
// re-fetches /pipelines right after the duplicate, so the copy lands in the
// block-1 button path at once. Both Copy affordances route through the same
// handleDuplicate seam, so they can never drift apart again.
describe("UnifiedLeftPanel duplicate is usable without reload (#371)", () => {
  const original: PipelineListEntry = {
    id: "planner",
    name: "planner",
    scope: "library",
    path: "/home/u/.pdo/library/pipelines/planner.yaml",
    node_count: 3,
    modified: null,
    variables: {},
  };
  // /pipelines shape of the copy: the daemon scans the library dir and tags it
  // scope:"library", so it belongs in block 1 (proper button, "library" badge).
  const copyPipe: PipelineListEntry = {
    ...original,
    id: "planner-copy",
    name: "planner (copy)",
    path: "/home/u/.pdo/library/pipelines/planner-copy.yaml",
  };
  // /library/pipelines shape of the same copy: raw storage scope "user" — the
  // value that rendered the degraded block-2 row before the fix.
  const copyLib: LibraryPipelineEntry = {
    id: "planner-copy",
    name: "planner (copy)",
    scope: "user",
    node_count: 3,
    modified: null,
    yaml: "name: planner (copy)\n",
    pipeline: { name: "planner (copy)", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
  };

  it("moves the copy from the degraded block-2 <div> to a proper block-1 button (block-1 Copy)", async () => {
    // /pipelines: only the original at mount; original + copy (both tagged
    // scope:"library") on the post-duplicate re-fetch the fix now performs.
    mockFetchPipelines.mockReset();
    mockFetchPipelines
      .mockResolvedValueOnce([original])
      .mockResolvedValue([original, copyPipe]);
    mockDuplicateLibraryPipeline.mockResolvedValueOnce({
      id: "planner-copy",
      scope: "user",
      entry: null,
    });

    // A stateful parent that mirrors App: onLibraryPipelinesChanged adds the
    // copy (raw "user" scope) to the /library/pipelines prop — exactly the
    // refresh that, ALONE, produced the degraded row before the fix.
    function Harness() {
      const [lib, setLib] = useState<LibraryPipelineEntry[]>([]);
      return (
        <UnifiedLeftPanel
          runs={[]}
          selectedRunId={null}
          onSelectRun={noop}
          onNewRun={noop}
          libraryPipelines={lib}
          onLibraryPipelinesChanged={() => setLib([copyLib])}
        />
      );
    }

    render(<Harness />);
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));
    await screen.findByText("planner");
    expect(screen.queryByText("planner (copy)")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("library-duplicate-button"));

    // The copy is now a real <button> (role) named for the copy, carrying the
    // "library" badge — not the degraded "user" <div>. Pre-fix this row stayed
    // a library-only <div>, so findByRole would time out.
    const copyRow = await screen.findByRole("button", { name: /planner \(copy\)/ });
    expect(within(copyRow).getByText("library")).toBeInTheDocument();
    // No degraded library-only row survives: once /pipelines carries the copy,
    // the block-2 name-absence filter drops it.
    expect(screen.queryByTestId("library-only-entry")).not.toBeInTheDocument();
  });

  it("surfaces the copy as a block-1 button when duplicating from a block-2 library-only row (block-2 Copy)", async () => {
    const solo: PipelineListEntry = {
      id: "solo",
      name: "solo",
      scope: "library",
      path: "/home/u/.pdo/library/pipelines/solo.yaml",
      node_count: 2,
      modified: null,
      variables: {},
    };
    const soloCopy: PipelineListEntry = {
      ...solo,
      id: "solo-copy",
      name: "solo (copy)",
      path: "/home/u/.pdo/library/pipelines/solo-copy.yaml",
    };
    const soloLibOnly: LibraryPipelineEntry = {
      id: "solo",
      name: "solo",
      scope: "user",
      node_count: 2,
      modified: null,
      yaml: "name: solo\n",
      pipeline: { name: "solo", version: "1.0", variables: {}, nodes: [], edges: [] },
      prompts: {},
    };
    // /pipelines empty at mount ⇒ `solo` renders as a block-2 library-only row;
    // the fix's re-fetch then returns both entries tagged scope:"library".
    mockFetchPipelines.mockReset();
    mockFetchPipelines
      .mockResolvedValueOnce([])
      .mockResolvedValue([solo, soloCopy]);
    mockDuplicateLibraryPipeline.mockResolvedValueOnce({
      id: "solo-copy",
      scope: "user",
      entry: null,
    });

    render(
      <UnifiedLeftPanel
        runs={[]}
        selectedRunId={null}
        onSelectRun={noop}
        onNewRun={noop}
        libraryPipelines={[soloLibOnly]}
        onLibraryPipelinesChanged={noop}
      />,
    );
    fireEvent.click(screen.getByRole("tab", { name: "Library" }));
    // The block-2 library-only row carries the duplicate affordance.
    await screen.findByTestId("library-only-entry");

    fireEvent.click(screen.getByTestId("library-duplicate-button"));

    // loadPipelines() (the block-2 handler now shares it) re-fetches /pipelines,
    // landing the copy in the clickable, "library"-badged block-1 path.
    const copyRow = await screen.findByRole("button", { name: /solo \(copy\)/ });
    expect(within(copyRow).getByText("library")).toBeInTheDocument();
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

describe("UnifiedLeftPanel — Open session (#316)", () => {
  const TERMINAL_NON_ARCHIVED: RunListEntry["status"][] = [
    "completed",
    "failed",
    "skipped",
    "halted",
  ];
  const HIDDEN: RunListEntry["status"][] = [
    "running",
    "awaiting_user",
    "paused",
    "archived",
  ];

  it.each(TERMINAL_NON_ARCHIVED)(
    "renders the open-session button for a %s run",
    (status) => {
      const runs: RunListEntry[] = [
        { run_id: "run-term-1", pipeline_name: "p", status, started_at: null, name: "R" },
      ];
      renderPanel({ runs });
      expect(screen.getByTestId("open-session-button")).toBeInTheDocument();
    },
  );

  it.each(HIDDEN)("hides the open-session button for a %s run", (status) => {
    const runs: RunListEntry[] = [
      { run_id: "run-x", pipeline_name: "p", status, started_at: null, name: "R" },
    ];
    renderPanel({ runs });
    expect(screen.queryByTestId("open-session-button")).not.toBeInTheDocument();
  });

  it("clicking open-session calls openRunShell and mounts the shell modal", async () => {
    mockOpenRunShell.mockResolvedValueOnce({ session: "pdo-shell-run-term-1", created: true });
    const runs: RunListEntry[] = [
      { run_id: "run-term-1", pipeline_name: "p", status: "failed", started_at: null, name: "R" },
    ];
    renderPanel({ runs });

    expect(screen.queryByTestId("run-shell-modal")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("open-session-button"));

    await waitFor(() => expect(mockOpenRunShell).toHaveBeenCalledWith("run-term-1"));
    const modal = await screen.findByTestId("run-shell-modal");
    expect(modal.getAttribute("data-session")).toBe("pdo-shell-run-term-1");
  });

  it("does not mount the shell modal when openRunShell rejects (silent, like cleanup)", async () => {
    mockOpenRunShell.mockRejectedValueOnce(new Error("409"));
    const runs: RunListEntry[] = [
      { run_id: "run-term-2", pipeline_name: "p", status: "completed", started_at: null, name: "R" },
    ];
    renderPanel({ runs });

    fireEvent.click(screen.getByTestId("open-session-button"));
    await waitFor(() => expect(mockOpenRunShell).toHaveBeenCalledWith("run-term-2"));
    expect(screen.queryByTestId("run-shell-modal")).not.toBeInTheDocument();
  });
});

// #110 — run rows expose status-gated lifecycle controls: Pause (live), Resume
// (paused), Retry-all (terminal, non-archived → confirm → archive + fresh run).
// Gating is on EXPLICIT statuses, never isLiveRun/isTerminalRun (which mis-include
// paused/archived respectively). Archived rows sit in the collapsed Archived
// section, so their row body is absent → queryByTestId is null (hidden), same as
// the #316 archived assertion.
describe("UnifiedLeftPanel — run-level controls (#110)", () => {
  const runRow = (status: RunListEntry["status"]): RunListEntry => ({
    run_id: "run-1",
    pipeline_name: "p",
    status,
    started_at: null,
    name: "R",
  });

  describe("Pause", () => {
    const VISIBLE: RunListEntry["status"][] = ["running", "awaiting_user"];
    const HIDDEN: RunListEntry["status"][] = [
      "paused",
      "completed",
      "failed",
      "halted",
      "skipped",
      "archived",
    ];

    it.each(VISIBLE)("renders the pause button for a %s run", (status) => {
      renderPanel({ runs: [runRow(status)] });
      expect(screen.getByTestId("pause-run-button")).toBeInTheDocument();
    });

    it.each(HIDDEN)("hides the pause button for a %s run", (status) => {
      renderPanel({ runs: [runRow(status)] });
      expect(screen.queryByTestId("pause-run-button")).not.toBeInTheDocument();
    });
  });

  describe("Resume", () => {
    const VISIBLE: RunListEntry["status"][] = ["paused"];
    const HIDDEN: RunListEntry["status"][] = [
      "running",
      "awaiting_user",
      "completed",
      "failed",
      "halted",
      "skipped",
      "archived",
    ];

    it.each(VISIBLE)("renders the resume button for a %s run", (status) => {
      renderPanel({ runs: [runRow(status)] });
      expect(screen.getByTestId("resume-run-button")).toBeInTheDocument();
    });

    it.each(HIDDEN)("hides the resume button for a %s run", (status) => {
      renderPanel({ runs: [runRow(status)] });
      expect(screen.queryByTestId("resume-run-button")).not.toBeInTheDocument();
    });
  });

  describe("Retry-all", () => {
    const VISIBLE: RunListEntry["status"][] = [
      "completed",
      "failed",
      "halted",
      "skipped",
    ];
    // NOT archived — the daemon 409s a retry_all on an archived run, and the row
    // is collapsed away anyway.
    const HIDDEN: RunListEntry["status"][] = [
      "running",
      "awaiting_user",
      "paused",
      "archived",
    ];

    it.each(VISIBLE)("renders the retry-all button for a %s run", (status) => {
      renderPanel({ runs: [runRow(status)] });
      expect(screen.getByTestId("retry-all-button")).toBeInTheDocument();
    });

    it.each(HIDDEN)("hides the retry-all button for a %s run", (status) => {
      renderPanel({ runs: [runRow(status)] });
      expect(screen.queryByTestId("retry-all-button")).not.toBeInTheDocument();
    });
  });

  it("clicking pause calls pauseRun with the run id", async () => {
    renderPanel({ runs: [runRow("running")] });
    fireEvent.click(screen.getByTestId("pause-run-button"));
    await waitFor(() => expect(mockPauseRun).toHaveBeenCalledWith("run-1"));
  });

  it("clicking resume calls resumeRun with the run id", async () => {
    renderPanel({ runs: [runRow("paused")] });
    fireEvent.click(screen.getByTestId("resume-run-button"));
    await waitFor(() => expect(mockResumeRun).toHaveBeenCalledWith("run-1"));
  });

  it("retry-all opens a confirm dialog without calling retryAll yet", () => {
    renderPanel({ runs: [runRow("completed")] });
    fireEvent.click(screen.getByTestId("retry-all-button"));

    expect(screen.getByTestId("retry-all-backdrop")).toBeInTheDocument();
    expect(screen.getByTestId("retry-all-confirm-button")).toBeInTheDocument();
    expect(mockRetryAll).not.toHaveBeenCalled();
  });

  it("confirming retry-all archives + creates a run and selects the offspring", async () => {
    mockRetryAll.mockResolvedValueOnce({ run_id: "offspring-1" });
    const onSelectRun = vi.fn();
    render(
      <UnifiedLeftPanel
        runs={[runRow("failed")]}
        selectedRunId={null}
        onSelectRun={onSelectRun}
        onNewRun={noop}
        libraryPipelines={[]}
        onLibraryPipelinesChanged={noop}
      />,
    );

    fireEvent.click(screen.getByTestId("retry-all-button"));
    fireEvent.click(screen.getByTestId("retry-all-confirm-button"));

    await waitFor(() => expect(mockRetryAll).toHaveBeenCalledWith("run-1"));
    await waitFor(() => expect(onSelectRun).toHaveBeenCalledWith("offspring-1"));
    // The modal closes once the flow resolves.
    await waitFor(() =>
      expect(screen.queryByTestId("retry-all-backdrop")).not.toBeInTheDocument(),
    );
  });

  it("cancelling retry-all never calls retryAll and closes the modal", () => {
    renderPanel({ runs: [runRow("halted")] });
    fireEvent.click(screen.getByTestId("retry-all-button"));
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(mockRetryAll).not.toHaveBeenCalled();
    expect(screen.queryByTestId("retry-all-backdrop")).not.toBeInTheDocument();
  });
});

// #336 — client-side run filters (Project / Pipeline / Trigger) above the Runs
// list. AND semantics, "All" default, session-only state. Options derive from
// the runs themselves (deleted pipelines/triggers stay filterable); the filter
// applies BEFORE the active/archived split so grouping and the Archived count
// both reflect it.
describe("UnifiedLeftPanel run filters (#336)", () => {
  const trig: Trigger = {
    id: "trg-1",
    name: "Nightly audit",
    pipeline_id: "auditor",
    pipeline_name: "auditor",
    input_template: "",
    variables: {},
    cron: "0 9 * * *",
    overlap_policy: "skip",
    enabled: true,
  };

  const runs: RunListEntry[] = [
    { run_id: "r1", pipeline_name: "auditor", status: "running", started_at: null, name: "Alpha auditor", effective_repo: "/repos/alpha", triggered_by: "trg-1" },
    { run_id: "r2", pipeline_name: "deploy", status: "completed", started_at: null, name: "Alpha deploy", effective_repo: "/repos/alpha" },
    { run_id: "r3", pipeline_name: "auditor", status: "running", started_at: null, name: "Zebra auditor", effective_repo: "/repos/zebra", triggered_by: "trg-gone" },
    { run_id: "r4", pipeline_name: "deploy", status: "archived", started_at: null, name: "Zebra archived", effective_repo: "/repos/zebra" },
  ];

  it("hides the filter row entirely when there are no runs", () => {
    renderPanel({ runs: [] });
    expect(screen.queryByTestId("run-filter-project")).not.toBeInTheDocument();
    expect(screen.queryByTestId("run-filter-pipeline")).not.toBeInTheDocument();
    expect(screen.queryByTestId("run-filter-trigger")).not.toBeInTheDocument();
  });

  it("renders the three dropdowns defaulting to All (placeholder labels)", () => {
    renderPanel({ runs, triggers: [trig] });
    expect(screen.getByTestId("run-filter-project")).toHaveTextContent("Project");
    expect(screen.getByTestId("run-filter-pipeline")).toHaveTextContent("Pipeline");
    expect(screen.getByTestId("run-filter-trigger")).toHaveTextContent("Trigger");
    // No clear control while everything is on "All".
    expect(screen.queryByTestId("run-filter-clear")).not.toBeInTheDocument();
  });

  it("filters by pipeline name", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });

    await user.click(screen.getByTestId("run-filter-pipeline"));
    await user.click(await screen.findByTestId("run-filter-option-deploy"));

    const labels = screen.getAllByTestId("run-display-label").map((el) => el.textContent);
    expect(labels).toEqual(["Alpha deploy"]);
    // Selected value shows on the dropdown trigger; clear control appears.
    expect(screen.getByTestId("run-filter-pipeline")).toHaveTextContent("deploy");
    expect(screen.getByTestId("run-filter-clear")).toBeInTheDocument();
  });

  it("filtering to a single repo flips the grouped list back to flat", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });
    // Two active repos ⇒ grouped before filtering.
    expect(screen.getAllByTestId("run-repo-group")).toHaveLength(2);

    await user.click(screen.getByTestId("run-filter-project"));
    await user.click(await screen.findByTestId("run-filter-option-/repos/alpha"));

    // One repo left ⇒ groupByRepo's ≥2 threshold fails ⇒ flat list.
    expect(screen.queryByTestId("run-repo-group")).not.toBeInTheDocument();
    const labels = screen.getAllByTestId("run-display-label").map((el) => el.textContent);
    expect(labels).toEqual(["Alpha auditor", "Alpha deploy"]);
  });

  it("filters by trigger, labelling options by trigger name with raw-id fallback", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });

    await user.click(screen.getByTestId("run-filter-trigger"));
    // Known trigger resolves to its name; deleted trigger falls back to the id.
    expect(await screen.findByTestId("run-filter-option-trg-1")).toHaveTextContent("Nightly audit");
    expect(screen.getByTestId("run-filter-option-trg-gone")).toHaveTextContent("trg-gone");

    await user.click(screen.getByTestId("run-filter-option-trg-1"));
    const labels = screen.getAllByTestId("run-display-label").map((el) => el.textContent);
    expect(labels).toEqual(["Alpha auditor"]);
  });

  it("offers a Manual option matching runs with no trigger", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });

    await user.click(screen.getByTestId("run-filter-trigger"));
    await user.click(await screen.findByTestId("run-filter-option-__manual__"));

    // r2 (active manual) visible; r4 (archived manual) counted in Archived.
    const labels = screen.getAllByTestId("run-display-label").map((el) => el.textContent);
    expect(labels).toEqual(["Alpha deploy"]);
    expect(screen.getByTestId("run-archived-count").textContent).toBe("(1)");
  });

  it("combines the three axes with AND semantics", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });

    await user.click(screen.getByTestId("run-filter-project"));
    await user.click(await screen.findByTestId("run-filter-option-/repos/alpha"));
    await user.click(screen.getByTestId("run-filter-pipeline"));
    await user.click(await screen.findByTestId("run-filter-option-auditor"));

    const labels = screen.getAllByTestId("run-display-label").map((el) => el.textContent);
    expect(labels).toEqual(["Alpha auditor"]);

    // A trigger choice contradicting the rest empties the list.
    await user.click(screen.getByTestId("run-filter-trigger"));
    await user.click(await screen.findByTestId("run-filter-option-trg-gone"));
    expect(screen.queryAllByTestId("run-display-label")).toHaveLength(0);
    expect(screen.getByTestId("run-filter-empty")).toBeInTheDocument();
  });

  it("applies the filter to the Archived section and its count", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });
    expect(screen.getByTestId("run-archived-count").textContent).toBe("(1)");

    // The archived run is a zebra deploy; filter to alpha ⇒ section disappears.
    await user.click(screen.getByTestId("run-filter-project"));
    await user.click(await screen.findByTestId("run-filter-option-/repos/alpha"));
    expect(screen.queryByTestId("run-archived-section")).not.toBeInTheDocument();

    // Filter to zebra ⇒ the section is back with the filtered count.
    await user.click(screen.getByTestId("run-filter-project"));
    await user.click(await screen.findByTestId("run-filter-option-/repos/zebra"));
    expect(screen.getByTestId("run-archived-count").textContent).toBe("(1)");
  });

  it("clears every axis via the clear control", async () => {
    const user = userEvent.setup();
    renderPanel({ runs, triggers: [trig] });

    await user.click(screen.getByTestId("run-filter-pipeline"));
    await user.click(await screen.findByTestId("run-filter-option-deploy"));
    expect(screen.getAllByTestId("run-display-label")).toHaveLength(1);

    await user.click(screen.getByTestId("run-filter-clear"));
    expect(screen.getAllByTestId("run-display-label")).toHaveLength(3);
    expect(screen.getByTestId("run-filter-pipeline")).toHaveTextContent("Pipeline");
    expect(screen.queryByTestId("run-filter-clear")).not.toBeInTheDocument();
  });

  it("shows the empty state with a working Clear-filters control on zero matches", async () => {
    const user = userEvent.setup();
    // Single-pipeline list plus a second pipeline elsewhere so both options exist.
    renderPanel({ runs, triggers: [trig] });

    await user.click(screen.getByTestId("run-filter-pipeline"));
    await user.click(await screen.findByTestId("run-filter-option-deploy"));
    await user.click(screen.getByTestId("run-filter-trigger"));
    await user.click(await screen.findByTestId("run-filter-option-trg-1"));

    expect(screen.getByTestId("run-filter-empty")).toBeInTheDocument();
    await user.click(screen.getByTestId("run-filter-empty-clear"));
    expect(screen.queryByTestId("run-filter-empty")).not.toBeInTheDocument();
    expect(screen.getAllByTestId("run-display-label")).toHaveLength(3);
  });

  it("buckets an empty pipeline_name without crashing", async () => {
    const user = userEvent.setup();
    const weird: RunListEntry[] = [
      { run_id: "w1", pipeline_name: "", status: "running", started_at: null, name: "Nameless", effective_repo: "/repos/a" },
      { run_id: "w2", pipeline_name: "real", status: "running", started_at: null, name: "Named", effective_repo: "/repos/a" },
    ];
    renderPanel({ runs: weird });

    await user.click(screen.getByTestId("run-filter-pipeline"));
    await user.click(await screen.findByTestId("run-filter-option-__none__"));
    const labels = screen.getAllByTestId("run-display-label").map((el) => el.textContent);
    expect(labels).toEqual(["Nameless"]);
  });
});
