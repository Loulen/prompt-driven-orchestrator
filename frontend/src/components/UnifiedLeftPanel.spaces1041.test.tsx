/**
 * #1041 — the Espace level of the Runs list: headers first, « No space » last,
 * the Projet grouping inside, a child under its root, drag-and-drop moves, the
 * archived swatch, and the « + Space » / pencil entry points.
 */
import { describe, it, expect, vi, beforeEach } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import UnifiedLeftPanel, { RUN_DRAG_TYPE } from "./UnifiedLeftPanel";
import type { RunListEntry, Space } from "../types";
import { setRunSpace } from "../api";
import { useSelectionStore } from "../stores/selectionStore";

vi.mock("../api", () => ({
  ApiError: class ApiError extends Error {},
  cleanupRun: vi.fn().mockResolvedValue(undefined),
  forgetRun: vi.fn().mockResolvedValue(undefined),
  pauseRun: vi.fn().mockResolvedValue(undefined),
  resumeRun: vi.fn().mockResolvedValue(undefined),
  retryAll: vi.fn().mockResolvedValue({ run_id: "x" }),
  renameRun: vi.fn().mockResolvedValue(undefined),
  openRunShell: vi.fn().mockResolvedValue({ session: "s", created: true }),
  fetchPipelines: vi.fn().mockResolvedValue([]),
  fetchProjects: vi.fn().mockResolvedValue([]),
  setRunSpace: vi.fn().mockResolvedValue({ run_id: "x", space_id: null }),
  createSpace: vi.fn().mockResolvedValue({ id: "spc-new", name: "New", color: "green" }),
  updateSpace: vi.fn().mockResolvedValue({ id: "spc-l", name: "Loïc", color: "blue" }),
  deleteSpace: vi.fn().mockResolvedValue(undefined),
}));

const noop = () => {};

const loic: Space = { id: "spc-l", name: "Loïc", color: "blue" };
const ana: Space = { id: "spc-a", name: "Ana", color: "orange" };

function run(id: string, over: Partial<RunListEntry> = {}): RunListEntry {
  return {
    run_id: id,
    pipeline_name: "pipe",
    status: "running",
    started_at: null,
    name: id,
    effective_repo: "/repos/front",
    ...over,
  };
}

function renderPanel(runs: RunListEntry[], spaces: Space[], onSpacesChanged = noop) {
  return render(
    <UnifiedLeftPanel
      runs={runs}
      selectedRunId={null}
      onSelectRun={noop}
      onNewRun={noop}
      spaces={spaces}
      onSpacesChanged={onSpacesChanged}
    />,
  );
}

/** A minimal `DataTransfer`: jsdom has none. */
function dataTransfer() {
  const data = new Map<string, string>();
  return {
    setData: (k: string, v: string) => void data.set(k, v),
    getData: (k: string) => data.get(k) ?? "",
    get types() {
      return [...data.keys()];
    },
    effectAllowed: "",
    dropEffect: "",
  };
}

function groupOf(spaceId: string): HTMLElement {
  const group = screen
    .getAllByTestId("run-space-group")
    .find((g) => g.getAttribute("data-space-id") === spaceId);
  if (!group) throw new Error(`no group for ${spaceId || "No space"}`);
  return group;
}

const rowIds = (el: HTMLElement) =>
  Array.from(el.querySelectorAll("[data-run-row]")).map((r) => r.getAttribute("data-run-row"));

beforeEach(() => {
  cleanup();
  vi.clearAllMocks();
  useSelectionStore.setState({ runs: [], library: [], triggers: [] });
});

describe("UnifiedLeftPanel — Espaces (#1041)", () => {
  it("leaves the list exactly as before while no Espace exists", () => {
    renderPanel([run("r1", { space_id: null }), run("r2")], []);
    expect(screen.queryByTestId("run-space-group")).not.toBeInTheDocument();
    expect(screen.queryByText("No space")).not.toBeInTheDocument();
    expect(rowIds(document.body)).toEqual(["r1", "r2"]);
    // Rows do not drag when there is nowhere to drop them.
    expect(document.querySelector("[data-run-row='r1']")).not.toHaveAttribute("draggable");
  });

  it("lists each Espace by name with its count, then No space last", () => {
    renderPanel(
      [run("r1", { space_id: "spc-l" }), run("r2"), run("r3", { space_id: "spc-a" })],
      [loic, ana],
    );
    const groups = screen.getAllByTestId("run-space-group");
    expect(groups.map((g) => within(g).getByTestId("run-space-label").textContent)).toEqual([
      "Ana",
      "Loïc",
      "No space",
    ]);
    expect(groups.map((g) => within(g).getByTestId("run-space-count").textContent)).toEqual([
      "1",
      "1",
      "1",
    ]);
    expect(rowIds(groupOf("spc-l"))).toEqual(["r1"]);
    expect(rowIds(groupOf(""))).toEqual(["r2"]);
    // The header carries the Espace's square swatch.
    expect(within(groupOf("spc-a")).getByTestId("run-space-header-swatch")).toHaveAttribute(
      "data-space-color",
      "orange",
    );
  });

  it("keeps the Projet / repo grouping inside each Espace", () => {
    renderPanel(
      [
        run("r1", { space_id: "spc-l", effective_repo: "/repos/front" }),
        run("r2", { space_id: "spc-l", effective_repo: "/repos/back" }),
        run("r3", { effective_repo: "/repos/front" }),
      ],
      [loic],
    );
    const loicGroups = within(groupOf("spc-l")).getAllByTestId("run-repo-group");
    expect(loicGroups.map((g) => within(g).getByTestId("run-repo-label").textContent)).toEqual([
      "back",
      "front",
    ]);
    // One repo alone in « No space »: the flat list, as before.
    expect(within(groupOf("")).queryByTestId("run-repo-group")).not.toBeInTheDocument();
  });

  it("shows a child Run under its root, in its root's Espace", () => {
    renderPanel(
      [
        run("root", { space_id: "spc-l" }),
        // Even an entry without its own Espace follows its root.
        run("child", { parent_run_id: "root", space_id: null }),
      ],
      [loic],
    );
    expect(rowIds(groupOf("spc-l"))).toEqual(["root", "child"]);
    expect(rowIds(groupOf(""))).toEqual([]);
    // Only the root drags: a child follows it.
    expect(document.querySelector("[data-run-row='root']")).toHaveAttribute("draggable", "true");
    expect(document.querySelector("[data-run-row='child']")).not.toHaveAttribute("draggable");
  });

  it("marks a filed row with its Espace's colour stripe", () => {
    renderPanel([run("r1", { space_id: "spc-l" }), run("r2")], [loic]);
    const filed = document.querySelector<HTMLElement>("[data-run-row='r1']")!;
    expect(filed.style.borderLeftColor).toBe("var(--color-space-blue)");
    expect(filed).toHaveAttribute("data-space-id", "spc-l");
    expect(document.querySelector<HTMLElement>("[data-run-row='r2']")!.style.borderLeftColor).toBe(
      "",
    );
  });

  it("moves a Run by dropping its row on another Espace header", async () => {
    const changed = vi.fn();
    renderPanel([run("r1"), run("r2", { space_id: "spc-l" })], [loic, ana], changed);
    const dt = dataTransfer();
    fireEvent.dragStart(document.querySelector("[data-run-row='r1']")!, { dataTransfer: dt });
    expect(dt.getData(RUN_DRAG_TYPE)).toBe("r1");

    const header = within(groupOf("spc-a")).getByTestId("run-space-header");
    fireEvent.dragOver(header, { dataTransfer: dt });
    expect(header).toHaveAttribute("data-drop-target", "true");
    fireEvent.drop(header, { dataTransfer: dt });

    await waitFor(() => expect(setRunSpace).toHaveBeenCalledWith("r1", "spc-a"));
    expect(changed).toHaveBeenCalled();
    expect(header).not.toHaveAttribute("data-drop-target");
  });

  it("takes a Run out of its Espace by dropping it on No space", async () => {
    renderPanel([run("r2", { space_id: "spc-l" })], [loic]);
    const dt = dataTransfer();
    fireEvent.dragStart(document.querySelector("[data-run-row='r2']")!, { dataTransfer: dt });
    fireEvent.drop(within(groupOf("")).getByTestId("run-space-header"), { dataTransfer: dt });
    await waitFor(() => expect(setRunSpace).toHaveBeenCalledWith("r2", null));
  });

  it("ignores a drop on the Espace the Run is already in, and foreign drags", () => {
    renderPanel([run("r2", { space_id: "spc-l" })], [loic]);
    const dt = dataTransfer();
    fireEvent.dragStart(document.querySelector("[data-run-row='r2']")!, { dataTransfer: dt });
    fireEvent.drop(within(groupOf("spc-l")).getByTestId("run-space-header"), { dataTransfer: dt });
    const foreign = dataTransfer();
    foreign.setData("text/plain", "hello");
    fireEvent.drop(within(groupOf("")).getByTestId("run-space-header"), { dataTransfer: foreign });
    expect(setRunSpace).not.toHaveBeenCalled();
  });

  it("collapses and expands an Espace", () => {
    renderPanel([run("r1", { space_id: "spc-l" })], [loic]);
    const toggle = within(groupOf("spc-l")).getByTestId("run-space-toggle");
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(rowIds(groupOf("spc-l"))).toEqual([]);
    fireEvent.click(toggle);
    expect(rowIds(groupOf("spc-l"))).toEqual(["r1"]);
  });

  it("keeps archived runs in their own section, each with its Espace's swatch", () => {
    renderPanel(
      [run("live", { space_id: "spc-l" }), run("old", { status: "archived", space_id: "spc-l" })],
      [loic],
    );
    expect(rowIds(groupOf("spc-l"))).toEqual(["live"]);
    fireEvent.click(screen.getByTestId("run-archived-toggle"));
    const archived = screen.getByTestId("run-archived-section");
    expect(rowIds(archived)).toEqual(["old"]);
    expect(within(archived).getByTestId("run-space-swatch")).toHaveAttribute(
      "aria-label",
      "Space: Loïc",
    );
    // A live row needs no swatch: its header already says where it is.
    expect(within(groupOf("spc-l")).queryByTestId("run-space-swatch")).not.toBeInTheDocument();
  });

  it("opens the Space dialog from « + Space » and from an Espace header's pencil", () => {
    renderPanel([run("r1", { space_id: "spc-l" })], [loic]);
    fireEvent.click(screen.getByTestId("new-space-button"));
    expect(screen.getByRole("dialog", { name: "New space" })).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("space-edit-cancel"));

    fireEvent.click(within(groupOf("spc-l")).getByTestId("run-space-pencil"));
    expect(screen.getByRole("dialog", { name: "Edit space" })).toBeInTheDocument();
    expect(screen.getByTestId("space-name-input")).toHaveValue("Loïc");
    // « No space » is not an Espace: no pencil.
    expect(within(groupOf("")).queryByTestId("run-space-pencil")).not.toBeInTheDocument();
  });
});
