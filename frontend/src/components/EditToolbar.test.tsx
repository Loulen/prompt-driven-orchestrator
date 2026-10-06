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
    // No Merge button (ADR-0079): convergence is wired onto any node.
    expect(screen.queryByTestId("toolbar-merge")).toBeNull();
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

  it("dropdown Node item calls onAddNode with agent (#307/#653)", async () => {
    const user = userEvent.setup();
    renderToolbar();
    await user.click(screen.getByTestId("toolbar-add"));
    await user.click(await screen.findByTestId("add-menu-node"));
    expect(onAddNode).toHaveBeenCalledWith("agent");
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

  it("script button calls onAddNode with script (#248)", () => {
    renderToolbar();
    expect(screen.getByTestId("toolbar-script")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("toolbar-script"));
    expect(onAddNode).toHaveBeenCalledWith("script");
  });

  it("tooltips render the correct text on hover", async () => {
    const user = userEvent.setup();
    renderToolbar();

    // #307: the `+` is now a dropdown trigger (no tooltip); the library/script
    // sibling buttons keep their tooltips.
    await user.hover(screen.getByTestId("toolbar-library"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Library");
    });

    fireEvent.pointerDown(screen.getByTestId("toolbar-library"));
    await waitFor(() => {
      expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument();
    });

    await user.hover(screen.getByTestId("toolbar-script"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent(
        "Script node (deterministic bash)",
      );
    });
  });

  // #397: the toolbar listed seven icon buttons in the a11y tree, six of them
  // anonymous — a Radix tooltip is a description, not a name (WCAG 4.1.2).
  describe("accessible names (#397)", () => {
    // Each name is the button's own visible tooltip text, verbatim.
    const NAMES: [string, string][] = [
      ["toolbar-add", "Add"],
      ["toolbar-library", "Library · L"],
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

  // #752 (CONTEXT.md « Accès rapide Review »): the Review quick access takes the
  // slot of the old "Run repositories" toggle (Repositories is a tab of the Run
  // panel now). A link, not a toggle; pill = pending count, tone = nuance.
  describe("Review quick access (#752)", () => {
    it("is absent on a non-run canvas (default)", () => {
      renderToolbar({ onToggleInfo: vi.fn() });
      expect(screen.queryByTestId("toolbar-review")).toBeNull();
      expect(screen.queryByTestId("toolbar-run-info")).toBeNull();
    });

    it("is a real link to the Review page, named, without aria-pressed", () => {
      renderToolbar({ reviewHref: "/runs/r1/review", reviewTitle: "Review" });
      const link = screen.getByTestId("toolbar-review");
      expect(link.tagName).toBe("A");
      expect(link).toHaveAttribute("href", "/runs/r1/review");
      expect(link).toHaveAccessibleName("Review");
      expect(link).not.toHaveAttribute("aria-pressed");
      expect(screen.queryByTestId("toolbar-review-pill")).toBeNull();
    });

    it("shows the pending count as an outlined pill, and the title spells it out", () => {
      renderToolbar({ reviewHref: "/runs/r1/review", reviewPending: 2, reviewTone: "pending", reviewTitle: "Review · 2 pending" });
      const link = screen.getByTestId("toolbar-review");
      expect(link).toHaveAccessibleName("Review · 2 pending");
      expect(link).toHaveAttribute("data-tone", "pending");
      expect(screen.getByTestId("toolbar-review-pill").textContent).toBe("2");
    });

    it("turns solid blue on an unread reply and amber on a proposed resolution", () => {
      const { unmount } = renderToolbar({ reviewHref: "/runs/r1/review", reviewPending: 2, reviewTone: "unread" });
      expect(screen.getByTestId("toolbar-review")).toHaveAttribute("data-tone", "unread");
      expect(screen.getByTestId("toolbar-review-pill").className).toContain("bg-st-running ");
      unmount();
      renderToolbar({ reviewHref: "/runs/r1/review", reviewPending: 2, reviewTone: "proposed" });
      expect(screen.getByTestId("toolbar-review")).toHaveAttribute("data-tone", "proposed");
      expect(screen.getByTestId("toolbar-review-pill").className).toContain("bg-st-await");
    });

    it("adds no button — the core count is unchanged — and the old toggle is gone", () => {
      renderToolbar({ reviewHref: "/runs/r1/review", onToggleInfo: vi.fn() });
      const buttons = [...screen.getByTestId("edit-toolbar").querySelectorAll("button")];
      expect(buttons).toHaveLength(6); // 6 core; Review is a link
      expect(screen.queryByTestId("toolbar-run-info")).toBeNull();
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
      expect(buttons).toHaveLength(7); // 6 core + assistant
      for (const b of buttons) expect(b).toHaveAccessibleName(/\S/);
    });

    // #938 (story PDO-3): `(i)` exposes its pressed state like the glyph does,
    // and the two render exactly what the host lights — one at a time.
    it("exposes aria-pressed on Pipeline info", () => {
      const onToggleInfo = vi.fn();
      const { unmount } = renderToolbar({ onToggleInfo });
      const info = screen.getByTestId("toolbar-info");
      expect(info).toHaveAccessibleName("Pipeline info");
      expect(info).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(info);
      expect(onToggleInfo).toHaveBeenCalledTimes(1);
      unmount();
      renderToolbar({ onToggleInfo, infoOpen: true });
      expect(screen.getByTestId("toolbar-info")).toHaveAttribute("aria-pressed", "true");
    });

    it.each([
      [{ assistantActive: true, infoOpen: false }, "true", "false"],
      [{ assistantActive: false, infoOpen: true }, "false", "true"],
      [{ assistantActive: false, infoOpen: false }, "false", "false"],
    ])("lights only the button the host marks active (%o)", (state, assistant, info) => {
      renderToolbar({
        assistantAvailable: true,
        onOpenAssistant: vi.fn(),
        onToggleInfo: vi.fn(),
        ...state,
      });
      const bot = screen.getByTestId("toolbar-assistant");
      const i = screen.getByTestId("toolbar-info");
      expect(bot).toHaveAttribute("aria-pressed", assistant);
      expect(i).toHaveAttribute("aria-pressed", info);
      expect(bot.className.includes("bg-acc text-bg-0")).toBe(assistant === "true");
      expect(i.className.includes("bg-acc text-bg-0")).toBe(info === "true");
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

describe("EditToolbar run « pilotage » and « Edit » menu (ADR-0080)", () => {
  function renderToolbar(props: Partial<ComponentProps<typeof EditToolbar>> = {}) {
    return render(
      <TooltipProvider>
        <EditToolbar
          onAddNode={vi.fn()}
          onAddNote={vi.fn()}
          onAddNodeFromYaml={vi.fn()}
          libraryEntries={[]}
          onLibraryDelete={vi.fn()}
          onToggleInfo={vi.fn()}
          {...props}
        />
      </TooltipProvider>,
    );
  }
  function runEdit(editing: boolean) {
    return {
      editing,
      onEditForRun: vi.fn(),
      onEditSource: vi.fn(),
      onFinishEditing: vi.fn(),
    };
  }

  it("in « pilotage » hides +, Library, script and undo/redo but keeps the run controls", () => {
    renderToolbar({
      readOnly: true,
      runEdit: runEdit(false),
      finishedRun: true,
      onReopen: vi.fn(),
      onRetryAll: vi.fn(),
      onOpenShell: vi.fn(),
      reviewHref: "/runs/r1/review",
    });
    for (const id of ["toolbar-add", "toolbar-library", "toolbar-script", "toolbar-undo", "toolbar-redo"]) {
      expect(screen.queryByTestId(id), id).toBeNull();
    }
    for (const id of ["toolbar-edit", "toolbar-reopen", "toolbar-retry-all", "toolbar-open-shell", "toolbar-review", "toolbar-info"]) {
      expect(screen.getByTestId(id), id).toBeInTheDocument();
    }
    expect(screen.queryByTestId("toolbar-finish-editing")).toBeNull();
  });

  it("the Edit menu offers « Edit for this run » and « Edit source pipeline »", async () => {
    const user = userEvent.setup();
    const edit = runEdit(false);
    renderToolbar({ readOnly: true, runEdit: edit });
    // A pencil icon only, named by its aria-label / title.
    const trigger = screen.getByTestId("toolbar-edit");
    expect(trigger).toHaveTextContent("");
    expect(trigger).toHaveAttribute("title", "Edit");
    expect(trigger).toHaveAccessibleName("Edit");
    await user.click(trigger);
    expect(await screen.findByTestId("toolbar-edit-for-run")).toHaveTextContent(/^Edit for this run$/);
    expect(screen.getByTestId("toolbar-edit-source")).toHaveTextContent(/^Edit source pipeline$/);
    await user.click(screen.getByTestId("toolbar-edit-for-run"));
    expect(edit.onEditForRun).toHaveBeenCalledTimes(1);

    await user.click(screen.getByTestId("toolbar-edit"));
    await user.click(await screen.findByTestId("toolbar-edit-source"));
    expect(edit.onEditSource).toHaveBeenCalledTimes(1);
  });

  it("while editing for the run, the authoring controls come back with « Finish editing »", () => {
    const edit = runEdit(true);
    renderToolbar({ readOnly: false, runEdit: edit });
    expect(screen.getByTestId("toolbar-add")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-undo")).toBeInTheDocument();
    expect(screen.queryByTestId("toolbar-edit")).toBeNull();
    fireEvent.click(screen.getByTestId("toolbar-finish-editing"));
    expect(edit.onFinishEditing).toHaveBeenCalledTimes(1);
  });

  it("an archived run gets no Edit control at all", () => {
    renderToolbar({ readOnly: true });
    expect(screen.queryByTestId("toolbar-edit")).toBeNull();
    expect(screen.queryByTestId("toolbar-finish-editing")).toBeNull();
    expect(screen.getByTestId("toolbar-info")).toBeInTheDocument();
  });
});
