import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ComponentProps } from "react";
import LibrarySubmenu from "./LibrarySubmenu";
import { DropdownMenu, DropdownMenuContent, DropdownMenuTrigger } from "../../shared/ui/dropdown-menu";
import type { LibraryEntry } from "../model/library";
import { useEditStore } from "../store/editStore";

// The submenu lives in the toolbar's `+` menu: mount it in a menu of its own.
async function openLibrary(props: ComponentProps<typeof LibrarySubmenu>) {
  const user = userEvent.setup();
  render(
    <DropdownMenu>
      <DropdownMenuTrigger data-testid="menu">+</DropdownMenuTrigger>
      <DropdownMenuContent>
        <LibrarySubmenu {...props} />
      </DropdownMenuContent>
    </DropdownMenu>,
  );
  await user.click(screen.getByTestId("menu"));
  await user.click(await screen.findByTestId("add-menu-library"));
  // From here on, no pointer travel: jsdom has no geometry, so the submenu's hover
  // guard would read any move off its trigger as leaving it. Rows take
  // `fireEvent.click`, the filter takes focus + keyboard.
  await screen.findByTestId("library-submenu");
  return user;
}

vi.mock("../../settings/api/settings", () => ({
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
// The agentic profiles' API answers with the mock of `../settings/api/settings` too.
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));
// The pipeline context's API answers with the mock of `../settings/api/settings` above.
vi.mock("../api/library", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));

vi.mock("../../lib/nanoid", () => ({
  generateNodeId: () => "mock-id",
}));

function makeEntry(name: string, prompt = "Some prompt"): LibraryEntry {
  return {
    name,
    type: "agent",
    inputs: [{ name: "in" }],
    outputs: [{ name: "out" }],
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

describe("LibrarySubmenu (CORE-194)", () => {
  it("is a `Library` submenu trigger of the menu it sits in", async () => {
    await openLibrary({ entries: [], onDelete: vi.fn() });
    expect(screen.getByTestId("add-menu-library")).toHaveAccessibleName("Library");
    expect(screen.getByTestId("add-menu-library")).toHaveAttribute("aria-haspopup", "menu");
  });

  it("shows empty state when no entries", async () => {
    await openLibrary({ entries: [], onDelete: vi.fn() });
    expect(screen.getByText("No saved node")).toBeInTheDocument();
  });

  it("shows entries and their count", async () => {
    await openLibrary({ entries: [makeEntry("Alpha"), makeEntry("Beta")], onDelete: vi.fn() });
    expect(screen.getByText("Alpha")).toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("filters entries as the user types, keys not swallowed by the menu typeahead", async () => {
    const user = await openLibrary({
      entries: [makeEntry("Reviewer"), makeEntry("Implementer")],
      onDelete: vi.fn(),
    });
    const search = screen.getByRole("textbox", { name: "Search nodes" });
    act(() => search.focus());
    await user.keyboard("review");
    expect(search).toHaveValue("review");
    expect(screen.getByText("Reviewer")).toBeInTheDocument();
    expect(screen.queryByText("Implementer")).not.toBeInTheDocument();

    await user.keyboard("zzz");
    expect(screen.getByText("No match")).toBeInTheDocument();
  });

  it("Escape clears a non-empty filter before closing", async () => {
    const user = await openLibrary({ entries: [makeEntry("Reviewer"), makeEntry("Implementer")], onDelete: vi.fn() });
    const search = screen.getByRole("textbox", { name: "Search nodes" });
    act(() => search.focus());
    await user.keyboard("rev");
    await user.keyboard("{Escape}");
    expect(search).toHaveValue("");
    expect(screen.getByTestId("library-submenu")).toBeInTheDocument();
    expect(screen.getByText("Implementer")).toBeInTheDocument();
  });

  it("Enter in the filter adds the first match and closes the menu", async () => {
    const user = await openLibrary({ entries: [makeEntry("Implementer"), makeEntry("Test")], onDelete: vi.fn() });
    const search = screen.getByRole("textbox", { name: "Search nodes" });
    act(() => search.focus());
    await user.keyboard("tes{Enter}");
    await waitFor(() => expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(1));
    await waitFor(() => expect(screen.queryByTestId("library-submenu")).toBeNull());
  });

  it("a letter typed on a row goes to the filter, ↑ on the first row reaches it", async () => {
    const user = await openLibrary({ entries: [makeEntry("Reviewer"), makeEntry("Implementer")], onDelete: vi.fn() });
    const search = screen.getByRole("textbox", { name: "Search nodes" });
    act(() => screen.getByTestId("library-entry-Reviewer").focus());
    await user.keyboard("r");
    expect(search).toHaveValue("r");
    expect(search).toHaveFocus();

    act(() => screen.getByTestId("library-entry-Reviewer").focus());
    await user.keyboard("{ArrowUp}");
    expect(search).toHaveFocus();
  });

  it("Delete on a row removes the entry from the library", async () => {
    const onDelete = vi.fn();
    const user = await openLibrary({ entries: [makeEntry("Alpha")], onDelete });
    act(() => screen.getByTestId("library-entry-Alpha").focus());
    await user.keyboard("{Delete}");
    expect(onDelete).toHaveBeenCalledWith("Alpha");
    expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(0);
  });

  it("shows prompt preview truncated to 60 chars", async () => {
    await openLibrary({ entries: [makeEntry("Node", "A".repeat(80))], onDelete: vi.fn() });
    expect(screen.getByText("A".repeat(60))).toBeInTheDocument();
  });

  it("has no 'Node from YAML…' row: it sits one level up, in the `+` menu", async () => {
    await openLibrary({ entries: [makeEntry("Alpha")], onDelete: vi.fn() });
    expect(screen.queryByTestId("library-add-node-from-yaml")).toBeNull();
    expect(screen.queryByText("Node from YAML…")).toBeNull();
  });

  it("clicking an entry adds it to the canvas and closes the menu", async () => {
    await openLibrary({ entries: [makeEntry("Test")], onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("library-entry-Test"));
    await waitFor(() => {
      const tab = useEditStore.getState().openTabs[0];
      expect(tab.pipeline.nodes).toHaveLength(1);
      expect(tab.prompts["mock-id"]).toBe("test prompt");
    });
    await waitFor(() => expect(screen.queryByTestId("library-submenu")).toBeNull());
  });

  it("the trash removes the entry from the library, adds nothing and keeps the menu open", async () => {
    const onDelete = vi.fn();
    await openLibrary({ entries: [makeEntry("Alpha")], onDelete });
    fireEvent.click(screen.getByRole("button", { name: "Remove from library" }));
    expect(onDelete).toHaveBeenCalledWith("Alpha");
    expect(useEditStore.getState().openTabs[0].pipeline.nodes).toHaveLength(0);
    expect(screen.getByTestId("library-submenu")).toBeInTheDocument();
  });

  // --- #655 / ADR-0060: the library is isolation-aware -----------------------

  it("names each entry's workspace in the preview", async () => {
    const entries = [
      makeEntry("Forker"),
      { ...makeEntry("Sharer"), isolated_worktree: false },
      { ...makeEntry("Marker"), type: "start" },
    ];
    await openLibrary({ entries, onDelete: vi.fn() });

    // A silent entry (pre-#655 on disk) still reads as the Agent default.
    expect(screen.getByTestId("library-workspace-isolated")).toHaveTextContent(
      "Isolated worktree",
    );
    expect(screen.getByTestId("library-workspace-shared")).toHaveTextContent("Run worktree");
    // A structural type carries no workspace, so the row shows no label to argue with.
    expect(screen.getAllByTestId(/^library-workspace-/)).toHaveLength(2);
    // …and each row wears its type's canvas glyph.
    expect(screen.getAllByTestId("node-icon-agent")).toHaveLength(2);
    expect(screen.getByTestId("node-icon-start")).toBeInTheDocument();
  });

  it("restores the entry's model, effort, workspace, on-demand and live artifacts onto the dropped node", async () => {
    const { instantiateFromLibrary } = await import("../api/library");
    vi.mocked(instantiateFromLibrary).mockResolvedValueOnce({
      spec: {
        name: "Sharer",
        type: "agent",
        inputs: [],
        outputs: [],
        interactive: false,
        model: "opus",
        effort: "high",
        isolated_worktree: false,
        on_demand: true,
      },
      prompt: "test prompt",
    });

    await openLibrary({ entries: [makeEntry("Sharer")], onDelete: vi.fn() });
    fireEvent.click(screen.getByTestId("library-entry-Sharer"));

    await vi.waitFor(() => {
      const tab = useEditStore.getState().openTabs[0];
      expect(tab.pipeline.nodes).toHaveLength(1);
      const node = tab.pipeline.nodes[0];
      // Not `undefined`: falling back to the type default would fork a
      // sub-worktree for an Agent its author had parked in the Run's.
      expect(node.isolated_worktree).toBe(false);
      expect(node.model).toBe("opus");
      expect(node.effort).toBe("high");
      expect(node.on_demand).toBe(true);
    });
  });

  it("says why the caller may not read the library, with no search nor count (CORE-248)", async () => {
    await openLibrary({
      entries: [makeEntry("Hidden")],
      refused: "You can't see this instance's library.",
      onDelete: vi.fn(),
    });
    expect(screen.getByTestId("library-refused")).toHaveTextContent("You can't see this instance's library.");
    expect(screen.queryByText("No saved node")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox", { name: "Search nodes" })).not.toBeInTheDocument();
    expect(screen.queryByText("1")).not.toBeInTheDocument();
    expect(screen.queryByText("Hidden")).not.toBeInTheDocument();
  });
});
