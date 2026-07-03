import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import EditToolbar from "./EditToolbar";
import { TooltipProvider } from "./ui/tooltip";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef } from "../types";
import type { TabHistory } from "../stores/editStore";

describe("EditToolbar", () => {
  const onAddNode = vi.fn();
  const onAddNote = vi.fn();
  const onLibraryDelete = vi.fn();

  beforeEach(() => {
    onAddNode.mockClear();
    onAddNote.mockClear();
    onLibraryDelete.mockClear();
  });

  function renderToolbar() {
    return render(
      <TooltipProvider>
        <EditToolbar
          onAddNode={onAddNode}
          onAddNote={onAddNote}
          libraryEntries={[]}
          onLibraryDelete={onLibraryDelete}
        />
      </TooltipProvider>,
    );
  }

  it("renders the core icon buttons", () => {
    renderToolbar();
    expect(screen.getByTestId("toolbar-add")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-library")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-merge")).toBeInTheDocument();
    // The Switch node was removed (ADR-0011): conditional routing now lives on
    // the edge, authored via the edge detail panel (#147).
    expect(screen.queryByTestId("toolbar-switch")).toBeNull();
    // The legacy Loop node was removed (#171): loops are expressed as a
    // `loops:` region, created by drawing a cycle (#166) — not a toolbar add.
    expect(screen.queryByTestId("toolbar-loop")).toBeNull();
  });

  it("add button opens a Node|Note dropdown (#307)", async () => {
    const user = userEvent.setup();
    renderToolbar();
    // The `+` is now a dropdown trigger, not a direct add — clicking it opens
    // the menu instead of immediately adding a node.
    await user.click(screen.getByTestId("toolbar-add"));
    expect(await screen.findByTestId("add-menu-node")).toBeInTheDocument();
    expect(screen.getByTestId("add-menu-note")).toBeInTheDocument();
    expect(onAddNode).not.toHaveBeenCalled();
    expect(onAddNote).not.toHaveBeenCalled();
  });

  it("dropdown Node item calls onAddNode with code-mutating (#307)", async () => {
    const user = userEvent.setup();
    renderToolbar();
    await user.click(screen.getByTestId("toolbar-add"));
    await user.click(await screen.findByTestId("add-menu-node"));
    expect(onAddNode).toHaveBeenCalledWith("code-mutating");
    expect(onAddNote).not.toHaveBeenCalled();
  });

  it("dropdown Note item calls onAddNote (#307)", async () => {
    const user = userEvent.setup();
    renderToolbar();
    await user.click(screen.getByTestId("toolbar-add"));
    await user.click(await screen.findByTestId("add-menu-note"));
    expect(onAddNote).toHaveBeenCalledTimes(1);
    expect(onAddNode).not.toHaveBeenCalled();
  });

  it("merge button calls onAddNode with merge", () => {
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-merge"));
    expect(onAddNode).toHaveBeenCalledWith("merge");
  });

  it("script button calls onAddNode with script (#248)", () => {
    renderToolbar();
    expect(screen.getByTestId("toolbar-script")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("toolbar-script"));
    expect(onAddNode).toHaveBeenCalledWith("script");
  });

  it("tooltips render the correct text on hover", async () => {
    const user = userEvent.setup();
    renderToolbar();

    // #307: the `+` is now a dropdown trigger (no tooltip); the library/merge
    // sibling buttons keep their tooltips.
    await user.hover(screen.getByTestId("toolbar-library"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Library");
    });

    fireEvent.pointerDown(screen.getByTestId("toolbar-library"));
    await waitFor(() => {
      expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument();
    });

    await user.hover(screen.getByTestId("toolbar-merge"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Merge node");
    });
  });
});

describe("EditToolbar undo/redo buttons (ADR-0014 / #226)", () => {
  const onAddNode = vi.fn();
  const onLibraryDelete = vi.fn();

  function pipe(): PipelineDef {
    return { name: "p", version: "1.0", variables: {}, nodes: [], edges: [] };
  }

  function seed(history: TabHistory) {
    useEditStore.setState({
      openTabs: [
        {
          id: "t",
          scope: "repo",
          pipeline: pipe(),
          prompts: {},
          diagnostics: [],
          dirty: false,
          externalDirty: false,
        },
      ],
      activeTabId: "t",
      selection: { kind: "none", id: null },
      history: { t: history },
    });
  }

  beforeEach(() => {
    onAddNode.mockClear();
    onLibraryDelete.mockClear();
    useEditStore.setState({ openTabs: [], activeTabId: null, history: {} });
  });

  function renderToolbar() {
    return render(
      <TooltipProvider>
        <EditToolbar onAddNode={onAddNode} onAddNote={vi.fn()} libraryEntries={[]} onLibraryDelete={onLibraryDelete} />
      </TooltipProvider>,
    );
  }

  it("renders both buttons with their testids", () => {
    seed({ past: [], future: [], lastKey: null, lastAt: 0 });
    renderToolbar();
    expect(screen.getByTestId("toolbar-undo")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-redo")).toBeInTheDocument();
  });

  it("both disabled when the history stacks are empty", () => {
    seed({ past: [], future: [], lastKey: null, lastAt: 0 });
    renderToolbar();
    expect(screen.getByTestId("toolbar-undo")).toBeDisabled();
    expect(screen.getByTestId("toolbar-redo")).toBeDisabled();
  });

  it("undo enabled when past is non-empty; redo enabled when future is non-empty", () => {
    seed({ past: [pipe()], future: [pipe()], lastKey: null, lastAt: 0 });
    renderToolbar();
    expect(screen.getByTestId("toolbar-undo")).toBeEnabled();
    expect(screen.getByTestId("toolbar-redo")).toBeEnabled();
  });

  it("clicking undo invokes the store's undo action", () => {
    const undoSpy = vi.fn();
    seed({ past: [pipe()], future: [], lastKey: null, lastAt: 0 });
    useEditStore.setState({ undo: undoSpy });
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-undo"));
    expect(undoSpy).toHaveBeenCalledTimes(1);
  });

  it("clicking redo invokes the store's redo action", () => {
    const redoSpy = vi.fn();
    seed({ past: [], future: [pipe()], lastKey: null, lastAt: 0 });
    useEditStore.setState({ redo: redoSpy });
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-redo"));
    expect(redoSpy).toHaveBeenCalledTimes(1);
  });

  it("a disabled undo button does not invoke the action", () => {
    const undoSpy = vi.fn();
    seed({ past: [], future: [], lastKey: null, lastAt: 0 });
    useEditStore.setState({ undo: undoSpy });
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-undo"));
    expect(undoSpy).not.toHaveBeenCalled();
  });
});
