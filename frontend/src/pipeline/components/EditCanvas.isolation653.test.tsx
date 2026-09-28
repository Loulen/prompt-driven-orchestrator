import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import EditCanvas from "./EditCanvas";
import { useEditStore } from "../store/editStore";
import { type OpenPipeline } from "../store/editState";
import { TooltipProvider } from "../../shared/ui/tooltip";

// jsdom has no ResizeObserver; ReactFlow's container measurement needs it.
globalThis.ResizeObserver = class {
  observe() {}
  unobserve() {}
  disconnect() {}
};

// The toolbar renders OUTSIDE <ReactFlow>, so collapsing the canvas body to a
// passthrough <div> leaves the add-node menu — the surface under test — intact.
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlow: (props: { children?: React.ReactNode }) => (
      <div data-testid="reactflow-stub">{props.children}</div>
    ),
  };
});

vi.mock("../../skill/api/bank", () => ({
  fetchSkillBank: vi.fn().mockResolvedValue({ skills: [], folders: [], root_path: "" }),
}));

vi.mock("../../settings/api/settings", () => ({
  fetchAgentProfiles: vi.fn().mockResolvedValue({ profiles: [] }),
  fetchLibrary: vi.fn().mockResolvedValue([]),
  fetchLibraryPipelines: vi.fn().mockResolvedValue([]),
  saveLibraryPipeline: vi.fn().mockResolvedValue({ id: "p", scope: "repo" }),
  deleteLibraryPipeline: vi.fn().mockResolvedValue(undefined),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
}));
// The agentic profiles' API answers with the mock of `../../settings/api/settings` too.
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
// The pipeline context's API answers with the mock of `../api` above.
vi.mock("../api/library", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));
vi.mock("../api/libraryPipelines", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));

vi.mock("../../workspace/api/projects", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../workspace/api/projects")>()),
  fetchProjects: vi.fn().mockResolvedValue([]),
}));

function emptyTab(): OpenPipeline {
  return {
    id: "p1",
    scope: "repo",
    pipeline: { name: "p1", version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
    libraryId: null,
    libraryScope: null,
  };
}

function renderCanvas() {
  return render(
    <TooltipProvider>
      <EditCanvas
        libraryEntries={[]}
        libraryPipelines={[]}
        onLibraryDelete={() => {}}
        onLibraryPipelinesChanged={() => {}}
      />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  useEditStore.setState({
    openTabs: [emptyTab()],
    activeTabId: "p1",
    selection: { kind: "none", id: null },
  });
});

/**
 * #653 / ADR-0060 — the editor's initial values. Isolated is the safe placement
 * for an Agent, so it is the default; a Script stays out of a sub-worktree so
 * lightweight runtime and artifact work stays lightweight. Both are written
 * down at creation, never left implicit.
 */
describe("EditCanvas — new-node isolation defaults (#653)", () => {
  it("creates an Agent isolated, and says so on the node", async () => {
    renderCanvas();
    fireEvent.click(screen.getByTestId("toolbar-add"));
    fireEvent.click(await screen.findByTestId("add-menu-node"));

    const nodes = useEditStore.getState().openTabs[0].pipeline.nodes;
    expect(nodes).toHaveLength(1);
    expect(nodes[0].type).toBe("agent");
    expect(nodes[0].isolated_worktree).toBe(true);
  });

  it("creates a Script in the Run worktree, and says so on the node", () => {
    renderCanvas();
    fireEvent.click(screen.getByTestId("toolbar-script"));

    const nodes = useEditStore.getState().openTabs[0].pipeline.nodes;
    expect(nodes).toHaveLength(1);
    expect(nodes[0].type).toBe("script");
    expect(nodes[0].isolated_worktree).toBe(false);
  });

  it("creates a Merge with no isolation to state — it forks by construction", () => {
    renderCanvas();
    fireEvent.click(screen.getByTestId("toolbar-merge"));

    const nodes = useEditStore.getState().openTabs[0].pipeline.nodes;
    expect(nodes).toHaveLength(1);
    expect(nodes[0].type).toBe("merge");
    expect(nodes[0].isolated_worktree).toBeUndefined();
  });
});
