import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import EditToolbar from "./EditToolbar";
import { TooltipProvider } from "./ui/tooltip";
import { useEditStore } from "../stores/editStore";
import type { PipelineDef } from "../types";
import type { TabHistory } from "../stores/editStore";

describe("EditToolbar", () => {
  const onAddNode = vi.fn();
  const onAddNote = vi.fn();
  const onAddNodeFromYaml = vi.fn();
  const onLibraryDelete = vi.fn();

  beforeEach(() => {
    onAddNode.mockClear();
    onAddNote.mockClear();
    onAddNodeFromYaml.mockClear();
    onLibraryDelete.mockClear();
  });

  function renderToolbar(props: Partial<ComponentProps<typeof EditToolbar>> = {}) {
    return render(
      <TooltipProvider>
        <EditToolbar
          onAddNode={onAddNode}
          onAddNote={onAddNote}
          onAddNodeFromYaml={onAddNodeFromYaml}
          libraryEntries={[]}
          onLibraryDelete={onLibraryDelete}
          {...props}
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

  it("dropdown has an 'Add node from YAML…' item that calls onAddNodeFromYaml (#345)", async () => {
    const user = userEvent.setup();
    renderToolbar();
    await user.click(screen.getByTestId("toolbar-add"));
    const item = await screen.findByTestId("add-menu-node-from-yaml");
    expect(item).toBeInTheDocument();
    await user.click(item);
    expect(onAddNodeFromYaml).toHaveBeenCalledTimes(1);
    expect(onAddNode).not.toHaveBeenCalled();
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

  // #397: the toolbar listed seven icon buttons in the a11y tree, six of them
  // anonymous — a Radix tooltip is a description, not a name (WCAG 4.1.2).
  describe("accessible names (#397)", () => {
    // Each name is the button's own visible tooltip text, verbatim.
    const NAMES: [string, string][] = [
      ["toolbar-add", "Add"],
      ["toolbar-library", "Library · L"],
      ["toolbar-merge", "Merge node"],
      ["toolbar-script", "Script node (deterministic bash)"],
      ["toolbar-undo", "Undo · Ctrl+Z"],
      ["toolbar-redo", "Redo · Ctrl+Y"],
      ["toolbar-info", "Pipeline info"],
    ];

    it.each(NAMES)("%s is named %j at rest", (testid, name) => {
      renderToolbar({ onToggleInfo: vi.fn() });
      // No hover, no focus — the name must hold in the resting state, which is
      // exactly where `aria-describedby` does not exist.
      expect(screen.getByTestId(testid)).toHaveAccessibleName(name);
    });

    it("leaves no anonymous button in the toolbar", () => {
      renderToolbar({ onToggleInfo: vi.fn() });
      const toolbar = screen.getByTestId("edit-toolbar");
      const buttons = [...toolbar.querySelectorAll("button")];
      expect(buttons).toHaveLength(NAMES.length);
      for (const b of buttons) expect(b).toHaveAccessibleName(/\S/);
    });

    it("undo/redo stay named while disabled", () => {
      renderToolbar();
      expect(screen.getByTestId("toolbar-undo")).toBeDisabled();
      expect(screen.getByTestId("toolbar-undo")).toHaveAccessibleName("Undo · Ctrl+Z");
      expect(screen.getByTestId("toolbar-redo")).toHaveAccessibleName("Redo · Ctrl+Y");
    });

    it("the read-only archived toolbar still names its lone info button (#315)", () => {
      renderToolbar({ readOnly: true, onToggleInfo: vi.fn() });
      const buttons = [...screen.getByTestId("edit-toolbar").querySelectorAll("button")];
      expect(buttons).toHaveLength(1);
      expect(buttons[0]).toHaveAccessibleName("Pipeline info");
    });
  });

  // #465 slice 2 (F1): the Run-info / Repositories toggle, shown only on the
  // live run tabs where the App auto-snaps to the running node — the exact set
  // where the sidebar is otherwise unreachable.
  describe("run-info toggle (#465 slice 2, F1)", () => {
    it("is absent on a non-run canvas (default)", () => {
      renderToolbar({ onToggleInfo: vi.fn() });
      expect(screen.queryByTestId("toolbar-run-info")).toBeNull();
    });

    it("stays absent when shown without a handler wired", () => {
      renderToolbar({ showRunInfo: true });
      expect(screen.queryByTestId("toolbar-run-info")).toBeNull();
    });

    it("renders named, unpressed at rest, and toggles on click", () => {
      const onToggleRunInfo = vi.fn();
      renderToolbar({ showRunInfo: true, onToggleRunInfo });
      const btn = screen.getByTestId("toolbar-run-info");
      expect(btn).toHaveAccessibleName("Run repositories");
      expect(btn).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(btn);
      expect(onToggleRunInfo).toHaveBeenCalledTimes(1);
    });

    it("reflects the pressed state while the sidebar is open", () => {
      renderToolbar({
        showRunInfo: true,
        onToggleRunInfo: vi.fn(),
        runInfoActive: true,
      });
      expect(screen.getByTestId("toolbar-run-info")).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    it("adds exactly one more named button beside Pipeline info", () => {
      renderToolbar({
        showRunInfo: true,
        onToggleRunInfo: vi.fn(),
        onToggleInfo: vi.fn(),
      });
      const buttons = [
        ...screen.getByTestId("edit-toolbar").querySelectorAll("button"),
      ];
      expect(buttons).toHaveLength(8); // 7 core + run-info
      for (const b of buttons) expect(b).toHaveAccessibleName(/\S/);
    });
  });

  // #302 / ADR-0048: the "agent" glyph beside `(i)`, shown only on a library
  // template canvas — the mirror of the run-info toggle above.
  describe("assistant toggle (#302)", () => {
    it("is absent on a canvas that is not a template (default)", () => {
      renderToolbar({ onToggleInfo: vi.fn() });
      expect(screen.queryByTestId("toolbar-assistant")).toBeNull();
    });

    it("stays absent when available but no handler is wired", () => {
      renderToolbar({ assistantAvailable: true });
      expect(screen.queryByTestId("toolbar-assistant")).toBeNull();
    });

    it("renders named, unpressed at rest, and opens on click", () => {
      const onOpenAssistant = vi.fn();
      renderToolbar({ assistantAvailable: true, onOpenAssistant });
      const btn = screen.getByTestId("toolbar-assistant");
      expect(btn).toHaveAccessibleName("Pipeline assistant");
      expect(btn).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(btn);
      expect(onOpenAssistant).toHaveBeenCalledTimes(1);
    });

    it("reflects the pressed state while the Assistant tab is the panel view", () => {
      renderToolbar({
        assistantAvailable: true,
        onOpenAssistant: vi.fn(),
        assistantActive: true,
      });
      expect(screen.getByTestId("toolbar-assistant")).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });

    it("adds exactly one more named button beside Pipeline info", () => {
      renderToolbar({
        assistantAvailable: true,
        onOpenAssistant: vi.fn(),
        onToggleInfo: vi.fn(),
      });
      const buttons = [
        ...screen.getByTestId("edit-toolbar").querySelectorAll("button"),
      ];
      expect(buttons).toHaveLength(8); // 7 core + assistant
      for (const b of buttons) expect(b).toHaveAccessibleName(/\S/);
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
        <EditToolbar onAddNode={onAddNode} onAddNote={vi.fn()} onAddNodeFromYaml={vi.fn()} libraryEntries={[]} onLibraryDelete={onLibraryDelete} />
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

describe("EditToolbar finished-run group (#598)", () => {
  function renderToolbar(props: Partial<ComponentProps<typeof EditToolbar>> = {}) {
    return render(
      <TooltipProvider>
        <EditToolbar
          onAddNode={vi.fn()}
          onAddNote={vi.fn()}
          onAddNodeFromYaml={vi.fn()}
          libraryEntries={[]}
          onLibraryDelete={vi.fn()}
          {...props}
        />
      </TooltipProvider>,
    );
  }

  it("hides the finished-run group on a live run", () => {
    renderToolbar({ finishedRun: false, onReopen: vi.fn() });
    expect(screen.queryByTestId("toolbar-reopen")).toBeNull();
    expect(screen.queryByTestId("toolbar-retry-all")).toBeNull();
    expect(screen.queryByTestId("toolbar-open-shell")).toBeNull();
  });

  it("shows Reopen/Retry-all/Open-shell on a terminal non-archived run and wires each", () => {
    const onReopen = vi.fn();
    const onRetryAll = vi.fn();
    const onOpenShell = vi.fn();
    renderToolbar({ finishedRun: true, onReopen, onRetryAll, onOpenShell });

    const reopen = screen.getByTestId("toolbar-reopen");
    const retry = screen.getByTestId("toolbar-retry-all");
    const shell = screen.getByTestId("toolbar-open-shell");
    expect(reopen).toBeInTheDocument();
    expect(retry).toBeInTheDocument();
    expect(shell).toBeInTheDocument();

    fireEvent.click(reopen);
    fireEvent.click(retry);
    fireEvent.click(shell);
    expect(onReopen).toHaveBeenCalledTimes(1);
    expect(onRetryAll).toHaveBeenCalledTimes(1);
    expect(onOpenShell).toHaveBeenCalledTimes(1);
  });

  it("renders only the buttons whose handlers are provided", () => {
    renderToolbar({ finishedRun: true, onReopen: vi.fn() });
    expect(screen.getByTestId("toolbar-reopen")).toBeInTheDocument();
    expect(screen.queryByTestId("toolbar-retry-all")).toBeNull();
    expect(screen.queryByTestId("toolbar-open-shell")).toBeNull();
  });
});
