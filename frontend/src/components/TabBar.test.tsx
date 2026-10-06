import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, beforeEach, vi } from "vitest";
import TabBar from "./TabBar";
import { fetchRunPipelineOverwritePreview, type OverwritePreview } from "../api";

vi.mock("../api", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api")>()),
  fetchRunPipelineOverwritePreview: vi.fn(),
}));
import { useEditStore } from "../stores/editStore";
import type { OpenPipeline } from "../stores/editStore";

function tab(id: string, over: Partial<OpenPipeline> = {}): OpenPipeline {
  return {
    id,
    scope: "repo",
    pipeline: { name: id, version: "1.0", variables: {}, nodes: [], edges: [] },
    prompts: {},
    diagnostics: [],
    dirty: false,
    externalDirty: false,
    ...over,
  };
}

function seed(tabs: OpenPipeline[], activeTabId: string) {
  useEditStore.setState({
    openTabs: tabs,
    activeTabId,
    selection: { kind: "none", id: null },
    history: {},
    singleTabMode: false,
    pendingSingleTab: null,
    lastSavedAt: {},
  });
}

/** Right-click the tab button that owns `tab-title-<id>`. */
function rightClickTab(id: string) {
  const btn = screen.getByTestId(`tab-title-${id}`).closest("button")!;
  fireEvent.contextMenu(btn);
}

beforeEach(() => {
  // jsdom doesn't implement scrollIntoView (called by TabBar's active-tab effect).
  Element.prototype.scrollIntoView = vi.fn();
});

describe("TabBar context menu (#342)", () => {
  it("renders one tab per open pipeline", () => {
    seed([tab("a"), tab("b")], "a");
    render(<TabBar />);
    expect(screen.getByTestId("tab-title-a")).toBeInTheDocument();
    expect(screen.getByTestId("tab-title-b")).toBeInTheDocument();
  });

  it("opens a 4-item menu on right-click", () => {
    seed([tab("a"), tab("b"), tab("c")], "a");
    render(<TabBar />);
    rightClickTab("b");
    expect(screen.getByTestId("tab-context-menu")).toBeInTheDocument();
    expect(screen.getByTestId("tab-ctx-close")).toBeInTheDocument();
    expect(screen.getByTestId("tab-ctx-close-others")).toBeInTheDocument();
    expect(screen.getByTestId("tab-ctx-close-right")).toBeInTheDocument();
    expect(screen.getByTestId("tab-ctx-close-all")).toBeInTheDocument();
  });

  it("disables 'Close to the right' on the last tab, enables it otherwise", () => {
    seed([tab("a"), tab("b"), tab("c")], "a");
    render(<TabBar />);
    rightClickTab("c");
    expect(screen.getByTestId("tab-ctx-close-right")).toBeDisabled();
    fireEvent.keyDown(document, { key: "Escape" });
    rightClickTab("a");
    expect(screen.getByTestId("tab-ctx-close-right")).not.toBeDisabled();
  });

  it("disables 'Close others' / 'Close all' when a single tab is open", () => {
    seed([tab("only")], "only");
    render(<TabBar />);
    rightClickTab("only");
    expect(screen.getByTestId("tab-ctx-close-others")).toBeDisabled();
    expect(screen.getByTestId("tab-ctx-close-all")).toBeDisabled();
    expect(screen.getByTestId("tab-ctx-close")).not.toBeDisabled();
  });

  it("Escape dismisses the menu without closing anything", () => {
    seed([tab("a"), tab("b")], "a");
    render(<TabBar />);
    rightClickTab("a");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(screen.queryByTestId("tab-context-menu")).not.toBeInTheDocument();
    expect(useEditStore.getState().openTabs).toHaveLength(2);
  });

  it("a backdrop click dismisses the menu", () => {
    seed([tab("a"), tab("b")], "a");
    const { container } = render(<TabBar />);
    rightClickTab("a");
    // The z-40 backdrop is the first fixed-inset div.
    const backdrop = container.querySelector(".fixed.inset-0")!;
    fireEvent.click(backdrop);
    expect(screen.queryByTestId("tab-context-menu")).not.toBeInTheDocument();
  });

  it("'Close all' on clean tabs closes everything with no confirmation", () => {
    seed([tab("a"), tab("b"), tab("c")], "a");
    render(<TabBar />);
    rightClickTab("a");
    fireEvent.click(screen.getByTestId("tab-ctx-close-all"));
    // No modal, and the store emptied.
    expect(screen.queryByTestId("close-tabs-confirm")).not.toBeInTheDocument();
    expect(useEditStore.getState().openTabs).toEqual([]);
    // Menu closed before any modal could show.
    expect(screen.queryByTestId("tab-context-menu")).not.toBeInTheDocument();
  });

  it("'Close others' with a dirty victim confirms first; confirm closes them", () => {
    seed([tab("a"), tab("b", { dirty: true }), tab("c")], "a");
    render(<TabBar />);
    rightClickTab("a");
    fireEvent.click(screen.getByTestId("tab-ctx-close-others"));

    // Menu gone, confirmation up, dirty victim named — nothing closed yet.
    expect(screen.queryByTestId("tab-context-menu")).not.toBeInTheDocument();
    expect(screen.getByTestId("close-tabs-confirm")).toBeInTheDocument();
    expect(screen.getByText("b.yaml")).toBeInTheDocument();
    expect(useEditStore.getState().openTabs).toHaveLength(3);

    fireEvent.click(screen.getByTestId("close-tabs-confirm"));
    expect(useEditStore.getState().openTabs.map((t) => t.id)).toEqual(["a"]);
  });

  it("'Close others' with a dirty victim can be cancelled, keeping every tab", () => {
    seed([tab("a"), tab("b", { dirty: true }), tab("c")], "a");
    render(<TabBar />);
    rightClickTab("a");
    fireEvent.click(screen.getByTestId("tab-ctx-close-others"));
    fireEvent.click(screen.getByTestId("close-tabs-cancel"));
    expect(useEditStore.getState().openTabs).toHaveLength(3);
    expect(useEditStore.getState().openTabs[1].dirty).toBe(true);
  });

  it("'Close others' on all-clean tabs closes immediately (no modal)", () => {
    seed([tab("a"), tab("b"), tab("c")], "a");
    render(<TabBar />);
    rightClickTab("a");
    fireEvent.click(screen.getByTestId("tab-ctx-close-others"));
    expect(screen.queryByTestId("close-tabs-confirm")).not.toBeInTheDocument();
    expect(useEditStore.getState().openTabs.map((t) => t.id)).toEqual(["a"]);
  });
});

describe("TabBar Save on a run tab (ADR-0080)", () => {
  const runTab = (over: Partial<OpenPipeline> = {}) =>
    tab("__run__r1", { scope: "run", runId: "r1", pipeline: { name: "source-pipe", version: "1.0", variables: {}, nodes: [], edges: [] }, ...over });
  const preview = (over: Partial<OverwritePreview> = {}): OverwritePreview => ({
    pipeline_id: "source-pipe",
    pipeline_exists: true,
    modified_since_launch: false,
    triggers: [],
    ...over,
  });

  it("in « pilotage » no Save button is shown, even when another tab is dirty", () => {
    seed([runTab(), tab("other", { dirty: true })], "__run__r1");
    render(<TabBar />);
    expect(screen.queryByTestId("save-button")).toBeNull();
    expect(screen.queryByTestId("save-menu")).toBeNull();
  });

  it("while editing for the run, Save reads « Save for this run » and saves the run only", () => {
    const save = vi.fn();
    seed([runTab({ runEditing: true, dirty: true })], "__run__r1");
    useEditStore.setState({ save });
    render(<TabBar />);
    const button = screen.getByTestId("save-button");
    expect(button).toHaveTextContent("Save for this run");
    fireEvent.click(button);
    expect(save).toHaveBeenCalledWith("__run__r1");
  });

  it("a template tab keeps its plain Save", () => {
    seed([tab("tpl", { dirty: true })], "tpl");
    render(<TabBar />);
    expect(screen.getByTestId("save-button")).toHaveTextContent(/^Save$/);
    expect(screen.queryByTestId("save-menu")).toBeNull();
  });

  it("« Overwrite default pipeline » warns about future runs, triggers and a change since launch", async () => {
    const user = userEvent.setup();
    vi.mocked(fetchRunPipelineOverwritePreview).mockResolvedValue(
      preview({
        modified_since_launch: true,
        triggers: [
          { id: "t1", name: "Nightly", enabled: true },
          { id: "t2", name: "Weekly", enabled: false },
        ],
      }),
    );
    const overwriteDefaultPipeline = vi.fn().mockResolvedValue(undefined);
    seed([runTab({ runEditing: true })], "__run__r1");
    useEditStore.setState({ overwriteDefaultPipeline });
    render(<TabBar />);

    await user.click(screen.getByTestId("save-menu"));
    await user.click(await screen.findByTestId("save-menu-overwrite"));

    expect(await screen.findByTestId("overwrite-default-modal")).toBeInTheDocument();
    expect(screen.getByTestId("overwrite-default-future-runs")).toHaveTextContent("All future runs will use it");
    expect(screen.getByTestId("overwrite-default-future-runs")).toHaveTextContent("source-pipe");
    const triggers = await screen.findAllByTestId("overwrite-default-trigger");
    expect(triggers.map((t) => t.textContent)).toEqual(["Nightly", "Weekly (disabled)"]);
    expect(screen.getByTestId("overwrite-default-modified")).toBeInTheDocument();
    expect(fetchRunPipelineOverwritePreview).toHaveBeenCalledWith("r1");

    await user.click(screen.getByTestId("overwrite-default-confirm"));
    expect(overwriteDefaultPipeline).toHaveBeenCalledWith("__run__r1");
    await waitFor(() => expect(screen.queryByTestId("overwrite-default-modal")).toBeNull());
  });

  it("says when no trigger launches the pipeline, and cancels without overwriting", async () => {
    const user = userEvent.setup();
    vi.mocked(fetchRunPipelineOverwritePreview).mockResolvedValue(preview());
    const overwriteDefaultPipeline = vi.fn();
    seed([runTab({ runEditing: true })], "__run__r1");
    useEditStore.setState({ overwriteDefaultPipeline });
    render(<TabBar />);
    await user.click(screen.getByTestId("save-menu"));
    await user.click(await screen.findByTestId("save-menu-overwrite"));
    expect(await screen.findByTestId("overwrite-default-triggers")).toHaveTextContent("No trigger launches this pipeline.");
    expect(screen.queryByTestId("overwrite-default-modified")).toBeNull();
    await user.click(screen.getByTestId("overwrite-default-cancel"));
    expect(screen.queryByTestId("overwrite-default-modal")).toBeNull();
    expect(overwriteDefaultPipeline).not.toHaveBeenCalled();
  });

  it("keeps the modal open with the reason when the overwrite fails", async () => {
    const user = userEvent.setup();
    vi.mocked(fetchRunPipelineOverwritePreview).mockResolvedValue(preview());
    const overwriteDefaultPipeline = vi.fn().mockRejectedValue(new Error("the default pipeline no longer exists"));
    seed([runTab({ runEditing: true })], "__run__r1");
    useEditStore.setState({ overwriteDefaultPipeline });
    render(<TabBar />);
    await user.click(screen.getByTestId("save-menu"));
    await user.click(await screen.findByTestId("save-menu-overwrite"));
    await screen.findByTestId("overwrite-default-triggers");
    await user.click(screen.getByTestId("overwrite-default-confirm"));
    expect(await screen.findByTestId("overwrite-default-error")).toHaveTextContent("no longer exists");
    expect(screen.getByTestId("overwrite-default-modal")).toBeInTheDocument();
  });
});
