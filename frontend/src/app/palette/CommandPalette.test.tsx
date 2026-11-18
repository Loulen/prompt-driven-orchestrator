import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../shared/generated/ExtensionEntry";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { UiCommand } from "../../shared/generated/UiCommand";
import type { UiContribution } from "../../shared/generated/UiContribution";
import { hostReactMajor } from "../../shared/extensions/shared";
import { resetExtensions, setBundleLoader, useExtensionsStore } from "../../shared/extensions/store";
import CommandField from "./CommandField";
import CommandPalette from "./CommandPalette";
import { usePaletteHost, type PaletteHost } from "./paletteHost";
import { usePalette } from "./paletteStore";

// CORE-206: the node's Stop and Retry the palette sends, once confirmed.
const { stopNode, retryNode, retryNodePreview } = vi.hoisted(() => ({
  stopNode: vi.fn(),
  retryNode: vi.fn(),
  retryNodePreview: vi.fn(),
}));
vi.mock("../../run/api/nodes", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  stopNode,
  retryNode,
  retryNodePreview,
}));

const run = (id: string, name: string, status = "running"): RunListEntry =>
  ({ run_id: id, name, pipeline_name: "implement-review", status, stalled: false, started_at: null, effective_repo: "/r" }) as RunListEntry;

const openRun = vi.fn();
const called: { context: unknown }[] = [];

function host(extra: Partial<PaletteHost> = {}): PaletteHost {
  return {
    runs: [run("r1", "Sort products by price"), run("r2", "Add a product search"), run("r3", "Persist the cart")],
    triggers: [],
    selection: null,
    openRun,
    openTrigger: () => {},
    ...extra,
  };
}

function extension(contributions: UiContribution[], commands: UiCommand[], id = "notes", name = "Notes"): ExtensionEntry {
  return {
    id,
    name,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands,
      locations: [],
    },
  };
}

const NOTES = extension(
  [
    { location: "palette", id: "new-note", label: "New note", type: "action", command: "notes.new" },
    { location: "palette", id: "purge", label: "Purge notes", type: "action", command: "notes.purge" },
    { location: "run/item/menu", id: "link", label: "Link to a note", type: "action", command: "notes.link" },
    { location: "palette", id: "search", label: "Notes", type: "search", search: "searchNotes" },
  ] as UiContribution[],
  [
    { id: "notes.new", title: "New note", export: "record" },
    { id: "notes.purge", title: "Purge notes", export: "record", operation: { resource: "notes/note", verb: "delete" } },
    { id: "notes.link", title: "Link to a note", export: "record" },
  ],
);

async function load(extensions: ExtensionEntry[], answers: Record<string, unknown> = {}, results?: unknown[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string, init?: RequestInit) => {
      if (url === "/ext") return Response.json(extensions);
      if (url in answers) return Response.json(answers[url]);
      if (url === "/authorize") {
        const { operations } = JSON.parse(String(init?.body));
        return Response.json({
          decisions: operations.map(() => ({ decision: "deny", reason: "no", sentence: "You can't purge notes. Ask an administrator." })),
        });
      }
      return new Response("not found", { status: 404 });
    }),
  );
  setBundleLoader(async () => ({
    record: (context: unknown) => void called.push({ context }),
    searchNotes: (query: string) =>
      results ?? [{ id: "1", ref: "N-1", title: `Note about ${query}`, meta: "Ops", link: "#/ext/notes/topbar/end/1" }],
  }));
  await useExtensionsStore.getState().load();
}

function renderPalette() {
  return render(
    <>
      <CommandField />
      <CommandPalette />
    </>,
  );
}

beforeEach(() => {
  resetExtensions();
  openRun.mockReset();
  called.length = 0;
  localStorage.clear();
  usePaletteHost.getState().setHost(host());
});

afterEach(() => {
  act(() => usePalette.getState().hide());
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("CommandPalette", () => {
  it("takes the focus when it opens and gives it back to the command field on Escape", () => {
    renderPalette();
    const field = screen.getByTestId("command-field");
    field.focus();
    fireEvent.click(field);
    const input = screen.getByTestId("command-palette-input");
    expect(document.activeElement).toBe(input);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByTestId("command-palette")).toBeNull();
    expect(document.activeElement).toBe(field);
  });

  it("gives the focus to the command field when nothing had it before Ctrl+K", () => {
    renderPalette();
    (document.activeElement as HTMLElement | null)?.blur();
    expect(document.activeElement).toBe(document.body);
    act(() => {
      fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    });
    fireEvent.keyDown(screen.getByTestId("command-palette-input"), { key: "Escape" });
    expect(document.activeElement).toBe(screen.getByTestId("command-field"));
  });

  it("keeps Tab in the box", () => {
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const input = screen.getByTestId("command-palette-input");
    fireEvent.keyDown(input, { key: "Tab" });
    expect(document.activeElement).toBe(input);
    fireEvent.keyDown(input, { key: "Tab", shiftKey: true });
    expect(document.activeElement).toBe(input);
  });

  it("puts the Run three letters match first, and opens it", () => {
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const input = screen.getByTestId("command-palette-input");
    fireEvent.change(input, { target: { value: "adp" } });
    const runs = screen.getByTestId("palette-group-runs");
    const [first] = within(runs).getAllByTestId("palette-row");
    expect(first).toHaveTextContent("Add a product search");
    expect(first).toHaveAttribute("aria-selected", "true");
    expect(input).toHaveAttribute("aria-activedescendant", first.id);
    fireEvent.keyDown(input, { key: "Enter" });
    expect(openRun).toHaveBeenCalledWith("r2");
    expect(screen.queryByTestId("command-palette")).toBeNull();
    // Remembered: the next empty query offers it again.
    fireEvent.click(screen.getByTestId("command-field"));
    expect(within(screen.getByTestId("palette-group-recent")).getByText("Add a product search")).toBeInTheDocument();
  });

  it("walks the rows with the arrows", () => {
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const input = screen.getByTestId("command-palette-input");
    fireEvent.change(input, { target: { value: "p" } });
    const rows = screen.getAllByRole("option");
    expect(rows[0]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(rows[1]).toHaveAttribute("aria-selected", "true");
    fireEvent.keyDown(input, { key: "ArrowUp" });
    fireEvent.keyDown(input, { key: "ArrowUp" });
    expect(rows[0]).toHaveAttribute("aria-selected", "true");
  });

  it("lists an extension's global Actions under its name, the refused one greyed with why, and runs the others", async () => {
    await load([NOTES]);
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const group = await screen.findByTestId("palette-group-ext:notes");
    expect(within(group).getByText("Notes")).toBeInTheDocument();
    expect(within(group).getByText("New note")).toBeInTheDocument();
    // `notes/note` × `delete` is refused: greyed, the sentence on its row (CORE-205), and picking it does nothing.
    const purge = (await within(group).findByText("Purge notes")).closest('[role="option"]')!;
    expect(purge).toHaveAttribute("aria-disabled", "true");
    expect(purge).toHaveTextContent("You can't purge notes. Ask an administrator.");
    fireEvent.click(purge);
    expect(called).toHaveLength(0);
    expect(screen.getByTestId("command-palette-input")).toBeInTheDocument();
    fireEvent.click(within(group).getByText("New note"));
    await waitFor(() => expect(called).toHaveLength(1));
    expect(called[0].context).toMatchObject({ location: "palette", selected: null, contribution: "new-note" });
  });

  it("offers the selected Run's row-menu Actions, on the Run", async () => {
    await load([NOTES]);
    usePaletteHost.getState().setHost(
      host({ selection: { kind: "run", run: { id: "r2", name: "Add a product search", pipeline: "p", status: "failed", attributes: {} } } }),
    );
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const selection = await screen.findByTestId("palette-group-selection");
    await within(selection).findByText("Link to a note");
    expect(within(selection).getByText("Copy run id")).toBeInTheDocument();
    expect(within(selection).getAllByText("Add a product search").length).toBeGreaterThan(0);
    fireEvent.click(within(selection).getByText("Link to a note"));
    await waitFor(() => expect(called).toHaveLength(1));
    expect(called[0].context).toMatchObject({ location: "run/item/menu", entity: { kind: "run", id: "r2" } });
  });

  it("shows a source's results in its extension's group and follows their link", async () => {
    await load([NOTES]);
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    fireEvent.change(screen.getByTestId("command-palette-input"), { target: { value: "cart" } });
    const result = await screen.findByTestId("palette-result");
    expect(result).toHaveTextContent("N-1");
    expect(result).toHaveTextContent("Note about cart");
    expect(screen.getByTestId("palette-group-ext:notes")).toContainElement(result);
    fireEvent.click(result);
    expect(window.location.hash).toBe("#/ext/notes/topbar/end/1");
  });

  it("draws a result's fixed status shape in its tone's colour, never spinning", async () => {
    const result = (id: string, tone: string, shape: string, label: string) => ({
      id,
      title: `Note ${id}`,
      status: { tone, shape, label },
      link: `#/ext/notes/topbar/end/${id}`,
    });
    await load([NOTES], {}, [
      result("1", "skipped", "todo", "To do"),
      result("2", "running", "doing", "In progress"),
      result("3", "awaiting", "review", "In review"),
      result("4", "done", "done", "Done"),
      { id: "5", title: "Note 5", status: { tone: "running", label: "Running" }, link: "#/ext/notes/topbar/end/5" },
    ]);
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    fireEvent.change(screen.getByTestId("command-palette-input"), { target: { value: "note" } });
    const rows = await screen.findAllByTestId("palette-result");
    const marks = rows.map((row) => row.querySelector("svg[role=img]")!);
    expect(marks.map((m) => m.getAttribute("aria-label"))).toEqual(["To do", "In progress", "In review", "Done", "Running"]);
    expect(marks.map((m) => m.getAttribute("stroke"))).toEqual([
      "var(--color-st-skipped)",
      "var(--color-st-running)",
      "var(--color-st-await)",
      "var(--color-st-done)",
      "var(--color-st-running)",
    ]);
    // Four distinct shapes, none spinning; a tone without shape keeps the tone's icon.
    expect(new Set(marks.slice(0, 4).map((m) => m.getAttribute("class"))).size).toBe(4);
    expect(marks.slice(0, 4).some((m) => m.classList.contains("d-spin"))).toBe(false);
    expect(marks[4].classList.contains("d-spin")).toBe(true);
  });

  it("shows a global Action's counter, and nothing at zero", async () => {
    const inbox = (count: number) => ({ count, accent: true, detail: `${count} unread` });
    const entry = extension(
      [{ location: "palette", id: "inbox", label: "Inbox", type: "action", command: "notes.inbox", counter: "/ext/notes/inbox" }] as UiContribution[],
      [{ id: "notes.inbox", title: "Inbox", export: "record" }],
    );
    await load([entry], { "/ext/notes/inbox": inbox(3) });
    const { unmount } = renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const group = await screen.findByTestId("palette-group-ext:notes");
    const count = await within(group).findByTestId("palette-count");
    expect(count).toHaveTextContent("3");
    expect(count).toHaveAttribute("title", "3 unread");
    expect(count).toHaveAttribute("data-accent", "true");
    act(() => usePalette.getState().hide());
    unmount();

    resetExtensions();
    await load([entry], { "/ext/notes/inbox": inbox(0) });
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    const again = await screen.findByTestId("palette-group-ext:notes");
    await waitFor(() => expect(vi.mocked(fetch)).toHaveBeenCalledWith("/ext/notes/inbox", expect.anything()));
    expect(within(again).getByText("Inbox")).toBeInTheDocument();
    expect(within(again).queryByTestId("palette-count")).toBeNull();
  });

  it("orders the extensions' groups by rank, whatever their registration order", async () => {
    const action = (ext: string, rank?: number) =>
      extension(
        [{ location: "palette", id: "open", label: `Open ${ext}`, type: "action", command: `${ext}.open`, rank }] as UiContribution[],
        [{ id: `${ext}.open`, title: `Open ${ext}`, export: "record" }],
        ext,
        ext.toUpperCase(),
      );
    await load([action("plain"), action("second", 2), action("first", 1)]);
    renderPalette();
    fireEvent.click(screen.getByTestId("command-field"));
    await screen.findByTestId("palette-group-ext:plain");
    const groups = screen.getAllByRole("group").map((g) => g.getAttribute("data-testid"));
    expect(groups.filter((g) => g?.startsWith("palette-group-ext:"))).toEqual([
      "palette-group-ext:first",
      "palette-group-ext:second",
      "palette-group-ext:plain",
    ]);
  });

  describe("a node's Stop asks first, in the node panel's window (CORE-206)", () => {
    const selectRunningNode = () =>
      usePaletteHost.getState().setHost(
        host({
          selection: {
            kind: "node",
            run: { id: "r1", status: "running" },
            node: { id: "implement", name: "implement", type: "agent", status: "running", iter: 1 },
          },
        }),
      );
    const pickStop = async () => {
      fireEvent.click(screen.getByTestId("command-field"));
      const selection = await screen.findByTestId("palette-group-selection");
      fireEvent.click(within(selection).getByText("Stop"));
    };

    beforeEach(() => {
      stopNode.mockReset().mockResolvedValue(undefined);
      act(() => usePalette.getState().settle());
    });

    it("closes the palette, then asks as the panel's Stop asks; cancel sends nothing", async () => {
      await load([]);
      selectRunningNode();
      renderPalette();
      await pickStop();
      expect(screen.queryByTestId("command-palette")).toBeNull();
      const dialog = screen.getByRole("dialog", { name: "Stop node implement?" });
      expect(within(dialog).getByTestId("gesture-confirm-line")).toHaveTextContent("Ends its agent session");
      expect(within(dialog).getByTestId("gesture-confirm-info")).toHaveAccessibleName("Ends this conversation");
      expect(within(dialog).getByRole("button", { name: "Stop" })).toHaveClass("d-btn--danger");
      fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(stopNode).not.toHaveBeenCalled();
    });

    it("stops the node once confirmed", async () => {
      await load([]);
      selectRunningNode();
      renderPalette();
      await pickStop();
      const dialog = screen.getByRole("dialog", { name: "Stop node implement?" });
      fireEvent.click(within(dialog).getByRole("button", { name: "Stop" }));
      expect(stopNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });

  describe("a node's Retry asks first when it destroys something, in the node panel's window (CORE-206)", () => {
    const selectNode = (status: string) =>
      usePaletteHost.getState().setHost(
        host({
          selection: {
            kind: "node",
            run: { id: "r1", status: "running" },
            node: { id: "implement", name: "implement", type: "agent", status, iter: 1 },
          } as PaletteHost["selection"],
        }),
      );
    const pickRetry = async () => {
      fireEvent.click(screen.getByTestId("command-field"));
      const selection = await screen.findByTestId("palette-group-selection");
      await act(async () => void fireEvent.click(within(selection).getByText("Retry")));
    };
    const preview = (affected: number) =>
      retryNodePreview.mockResolvedValue({ downstream: [], affected_count: affected, with_artifacts: [] });

    beforeEach(() => {
      retryNode.mockReset().mockResolvedValue({ ok: true, iter: 2, invalidated: [] });
      retryNodePreview.mockReset();
      act(() => usePalette.getState().settle());
    });

    it("on a running node, closes the palette and says it ends its live session, in the danger tone; cancel sends nothing", async () => {
      preview(0);
      await load([]);
      selectNode("running");
      renderPalette();
      await pickRetry();
      expect(screen.queryByTestId("command-palette")).toBeNull();
      const dialog = await screen.findByRole("dialog", { name: "Retry node implement?" });
      expect(within(dialog).getAllByTestId("gesture-confirm-line").map((l) => l.textContent)).toEqual([
        "Ends its live session",
      ]);
      expect(within(dialog).getByRole("button", { name: "Retry" })).toHaveClass("d-btn--danger");
      fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));
      expect(screen.queryByRole("dialog")).toBeNull();
      expect(retryNode).not.toHaveBeenCalled();
    });

    it("on a completed node whose downstream holds artifacts, says how many it resets, in the danger tone; confirm retries", async () => {
      preview(2);
      await load([]);
      selectNode("completed");
      renderPalette();
      await pickRetry();
      const dialog = await screen.findByRole("dialog", { name: "Retry node implement?" });
      expect(within(dialog).getByTestId("gesture-confirm-line")).toHaveTextContent("Resets 2 downstream nodes");
      expect(within(dialog).getByRole("button", { name: "Retry" })).toHaveClass("d-btn--danger");
      fireEvent.click(within(dialog).getByRole("button", { name: "Retry" }));
      expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "implement");
      expect(screen.queryByRole("dialog")).toBeNull();
    });

    it("on a failed node with nothing downstream, retries at once, no window", async () => {
      preview(0);
      await load([]);
      selectNode("failed");
      renderPalette();
      await pickRetry();
      await waitFor(() => expect(retryNode).toHaveBeenCalledExactlyOnceWith("r1", "implement"));
      expect(screen.queryByRole("dialog")).toBeNull();
    });
  });
});
