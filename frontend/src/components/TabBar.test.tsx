import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import TabBar from "./TabBar";
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
    // Re-open on a non-last tab.
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
