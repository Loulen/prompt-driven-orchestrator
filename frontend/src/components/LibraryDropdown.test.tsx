import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import LibraryDropdown from "./LibraryDropdown";
import type { LibraryEntry } from "../api";
import { useEditStore } from "../stores/editStore";
import { TooltipProvider } from "./ui/tooltip";

function renderDropdown(props: Parameters<typeof LibraryDropdown>[0]) {
  return render(
    <TooltipProvider>
      <LibraryDropdown {...props} />
    </TooltipProvider>,
  );
}

vi.mock("../api", () => ({
  fetchLibrary: vi.fn().mockResolvedValue([]),
  saveToLibrary: vi.fn().mockResolvedValue({}),
  deleteFromLibrary: vi.fn().mockResolvedValue(undefined),
  instantiateFromLibrary: vi.fn().mockResolvedValue({
    spec: {
      name: "Test",
      type: "agent",
      inputs: [],
      outputs: [],
      interactive: false,
    },
    prompt: "test prompt",
  }),
}));

vi.mock("../lib/nanoid", () => ({
  generateNodeId: () => "mock-id",
}));

function makeEntry(name: string, prompt = "Some prompt"): LibraryEntry {
  return {
    name,
    type: "agent",
    inputs: [{ name: "in", repeated: false }],
    outputs: [{ name: "out", repeated: false }],
    interactive: false,
    prompt,
  };
}

beforeEach(() => {
  useEditStore.setState({
    openTabs: [
      {
        id: "test-tab",
        scope: "repo",
        pipeline: {
          name: "test",
          version: "1.0",
          variables: {},
          nodes: [],
          edges: [],
        },
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      },
    ],
    activeTabId: "test-tab",
    selection: { kind: "none", id: null },
  });
});

describe("LibraryDropdown", () => {
  it("renders the library button", () => {
    renderDropdown({ entries: [], onDelete: vi.fn() });
    expect(screen.getByTestId("toolbar-library")).toBeInTheDocument();
  });

  it("shows empty state when no entries and dropdown opened", () => {
    renderDropdown({ entries: [], onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    expect(screen.getByText(/No saved nodes yet/)).toBeInTheDocument();
  });

  it("shows entries when dropdown is opened", () => {
    const entries = [makeEntry("Alpha"), makeEntry("Beta")];
    renderDropdown({ entries: entries, onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
  });

  it("filters entries by search", () => {
    const entries = [makeEntry("Reviewer"), makeEntry("Implementer")];
    renderDropdown({ entries: entries, onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    const searchInput = screen.getByPlaceholderText("Search nodes...");
    fireEvent.change(searchInput, { target: { value: "review" } });
    expect(screen.getByText("Reviewer")).toBeInTheDocument();
    expect(screen.queryByText("Implementer")).not.toBeInTheDocument();
  });

  it("shows entry count in header", () => {
    const entries = [makeEntry("A"), makeEntry("B"), makeEntry("C")];
    renderDropdown({ entries: entries, onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    expect(screen.getByText("3 entries")).toBeInTheDocument();
  });

  it("shows singular count for 1 entry", () => {
    const entries = [makeEntry("Solo")];
    renderDropdown({ entries: entries, onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    expect(screen.getByText("1 entry")).toBeInTheDocument();
  });

  it("shows prompt preview truncated to 60 chars", () => {
    const longPrompt = "A".repeat(80);
    const entries = [makeEntry("Node", longPrompt)];
    renderDropdown({ entries: entries, onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    expect(screen.getByText("A".repeat(60))).toBeInTheDocument();
  });

  it("offers 'Add node from YAML…' when the callback is provided, and fires it (#345)", () => {
    const onAddNodeFromYaml = vi.fn();
    renderDropdown({ entries: [], onDelete: vi.fn(), onAddNodeFromYaml });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    const entry = screen.getByTestId("library-add-node-from-yaml");
    expect(entry).toBeInTheDocument();
    fireEvent.click(entry);
    expect(onAddNodeFromYaml).toHaveBeenCalledTimes(1);
  });

  it("omits the 'Add node from YAML…' entry when no callback is given", () => {
    renderDropdown({ entries: [], onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    expect(screen.queryByTestId("library-add-node-from-yaml")).toBeNull();
  });

  // --- #655 / ADR-0060: the library is isolation-aware -----------------------

  it("names each entry's workspace in the preview", () => {
    const entries = [
      makeEntry("Forker"),
      { ...makeEntry("Sharer"), isolated_worktree: false },
      { ...makeEntry("Gatherer"), type: "merge" },
    ];
    renderDropdown({ entries, onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));

    // A silent entry (pre-#655 on disk) still reads as the Agent default.
    expect(screen.getByTestId("library-workspace-isolated")).toHaveTextContent(
      "Isolated worktree",
    );
    expect(screen.getByTestId("library-workspace-shared")).toHaveTextContent("Run worktree");
    // A Merge carries no workspace, so the row shows no label to argue with.
    expect(screen.getAllByTestId(/^library-workspace-/)).toHaveLength(2);
    // …and each row wears its type's canvas glyph rather than a two-letter
    // badge that only knew `agent`.
    expect(screen.getAllByTestId("node-icon-agent")).toHaveLength(2);
    expect(screen.getByTestId("node-icon-merge")).toBeInTheDocument();
  });

  it("restores the entry's workspace onto the dropped node", async () => {
    const { instantiateFromLibrary } = await import("../api");
    vi.mocked(instantiateFromLibrary).mockResolvedValueOnce({
      spec: {
        name: "Sharer",
        type: "agent",
        inputs: [],
        outputs: [],
        interactive: false,
        isolated_worktree: false,
      },
      prompt: "test prompt",
    });

    renderDropdown({ entries: [makeEntry("Sharer")], onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("toolbar-library"));
    fireEvent.mouseEnter(screen.getByText("Sharer").closest("div.group")!);
    fireEvent.click(screen.getByTitle("Add to canvas"));

    await vi.waitFor(() => {
      const tab = useEditStore.getState().openTabs[0];
      expect(tab.pipeline.nodes).toHaveLength(1);
      // Not `undefined`: falling back to the type default would fork a
      // sub-worktree for an Agent its author had parked in the Run's.
      expect(tab.pipeline.nodes[0].isolated_worktree).toBe(false);
    });
  });
});
