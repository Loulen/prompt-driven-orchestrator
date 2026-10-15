import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within, waitFor } from "@testing-library/react";
import type { RunState } from "../types";
import { useEditStore } from "../stores/editStore";
import { useWiringStore } from "../stores/wiringStore";
import { serializePipeline } from "../lib/serializePipeline";

// Notion #6 / #948: the Info tab of Pipeline info is the only surface of a
// pipeline's metadata — these cases were the standalone Pipeline Inspector's.
vi.mock("./DiffTab", () => ({ default: () => null }));
vi.mock("./TmuxTerminal", () => ({ default: () => null }));
vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return {
    ...actual,
    // The Run view reads its drift on mount; unreadable here → no Source block.
    fetchSourceDrift: vi.fn().mockRejectedValue(new Error("no drift in this test")),
    fetchRemotes: vi.fn(),
    fetchRunPipelineDocument: vi.fn().mockRejectedValue(new Error("offline")),
    fetchPipelineDocument: vi.fn().mockRejectedValue(new Error("offline")),
  };
});

import PipelineInfoPanel from "./PipelineInfoPanel";
import { fetchPipelineDocument } from "../api";

function seedTab() {
  useEditStore.setState({
    openTabs: [
      {
        id: "p1",
        scope: "repo",
        pipeline: {
          name: "My Pipeline",
          version: "1.0",
          variables: {},
          nodes: [
            {
              id: "start",
              name: "Start",
              type: "start",
              interactive: false,
              inputs: [],
              outputs: [{ name: "user_prompt" }],
            },
            {
              id: "end",
              name: "End",
              type: "end",
              interactive: false,
              inputs: [{ name: "result" }],
              outputs: [],
            },
          ],
          edges: [
            {
              source: { node: "start", port: "user_prompt" },
              target: { node: "end", port: "result" },
            },
          ],
        },
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
        libraryId: null,
        libraryScope: null,
      },
    ],
    activeTabId: "p1",
    history: {},
    selection: { kind: "none", id: null },
  });
}

const pipelineNow = () => useEditStore.getState().openTabs[0].pipeline;
const tabNow = () => useEditStore.getState().openTabs[0];

function patchPipeline(patch: Record<string, unknown>) {
  useEditStore.setState((s) => ({
    openTabs: s.openTabs.map((t) => ({ ...t, pipeline: { ...t.pipeline, ...patch } })),
  }));
}

/** Mirrors App: the panel is fed the active edit tab's pipeline. */
function StorePanel({ run = null, assistantId }: { run?: RunState | null; assistantId?: string }) {
  const pipeline = useEditStore((s) => s.openTabs.find((t) => t.id === s.activeTabId)?.pipeline ?? null);
  return <PipelineInfoPanel run={run} pipeline={pipeline} onClose={() => {}} assistantId={assistantId} />;
}

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    run_id: "run-abc1234567",
    status: "running",
    pipeline_name: "My Pipeline",
    name: null,
    input: "do the thing",
    started_at: "2026-07-01T10:00:00.000Z",
    completed_at: null,
    nodes: {},
    edges: [],
    node_defs: [],
    start_node: null,
    end_node: null,
    merge_resolver: null,
    ...overrides,
  };
}

beforeEach(() => {
  seedTab();
  useWiringStore.setState({ defaultGridSize: "M" });
});

describe("Pipeline info — Info tab on a template (#948)", () => {
  it("shows header → Identity → Variables → Canvas → Stats → note, and no Description block", () => {
    render(<StorePanel />);
    const panel = screen.getByTestId("pipeline-info-panel");
    const order = [
      screen.getByTestId("info-panel-name"),
      screen.getByTestId("pipeline-meta-identity"),
      screen.getByTestId("info-panel-variables"),
      screen.getByTestId("pipeline-meta-canvas"),
      screen.getByTestId("info-stats"),
      screen.getByText(/The Manager tab becomes available/),
    ];
    for (let i = 1; i < order.length; i++) {
      expect(order[i - 1].compareDocumentPosition(order[i]) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    expect(within(panel).queryByText("Description")).not.toBeInTheDocument();
    expect(within(panel).queryByText(/Pipeline: My Pipeline/)).not.toBeInTheDocument();
    expect(screen.getByTestId("pipeline-graph-stats")).toHaveTextContent("2 nodes");
    expect(screen.getByTestId("pipeline-graph-stats")).toHaveTextContent("1 edges");
    expect(screen.queryByText("Pipeline Inspector")).not.toBeInTheDocument();
  });

  it("edits name and version through the edit store: dirty, undoable", () => {
    render(<StorePanel />);
    fireEvent.change(screen.getByTestId("pipeline-name-input"), { target: { value: "Renamed" } });
    expect(pipelineNow().name).toBe("Renamed");
    expect(tabNow().dirty).toBe(true);
    expect(screen.getByTestId("info-panel-name")).toHaveTextContent("Renamed");

    fireEvent.change(screen.getByTestId("pipeline-version-input"), { target: { value: "2.0" } });
    expect(pipelineNow().version).toBe("2.0");
    expect(serializePipeline(pipelineNow())).toContain("Renamed");

    useEditStore.getState().undo();
    expect(pipelineNow().version).toBe("1.0");
  });

  it("checks 'Prompt required' by default when the flag is absent", () => {
    render(<StorePanel />);
    expect((screen.getByTestId("prompt-required-checkbox") as HTMLInputElement).checked).toBe(true);
  });

  it("unchecks 'Prompt required' when the pipeline is prompt-optional", () => {
    patchPipeline({ prompt_required: false });
    render(<StorePanel />);
    expect((screen.getByTestId("prompt-required-checkbox") as HTMLInputElement).checked).toBe(false);
  });

  it("toggling the checkbox writes prompt_required to the pipeline", () => {
    render(<StorePanel />);
    const checkbox = screen.getByTestId("prompt-required-checkbox");
    fireEvent.click(checkbox);
    expect(pipelineNow().prompt_required).toBe(false);
    expect(serializePipeline(pipelineNow())).toContain("prompt_required: false");
    fireEvent.click(checkbox);
    expect(pipelineNow().prompt_required).toBe(true);
  });

  it("adds, renames, retypes, sets the default of and deletes a variable", () => {
    render(<StorePanel />);
    fireEvent.click(screen.getByTestId("pipeline-variable-add"));
    expect(pipelineNow().variables).toEqual({ new_var: { type: "int", default: 0 } });
    fireEvent.click(screen.getByTestId("pipeline-variable-add"));
    expect(Object.keys(pipelineNow().variables)).toEqual(["new_var", "new_var_2"]);

    const firstRow = () => screen.getAllByTestId("pipeline-variable-row")[0];
    fireEvent.change(within(firstRow()).getByTestId("pipeline-variable-name"), { target: { value: "max_iter" } });
    expect(pipelineNow().variables.max_iter).toEqual({ type: "int", default: 0 });
    expect(pipelineNow().variables.new_var).toBeUndefined();

    const row = screen.getAllByTestId("pipeline-variable-row").find(
      (r) => (within(r).getByTestId("pipeline-variable-name") as HTMLInputElement).value === "max_iter",
    )!;
    fireEvent.change(within(row).getByTestId("pipeline-variable-default"), { target: { value: "5" } });
    expect(pipelineNow().variables.max_iter.default).toBe(5);
    fireEvent.change(within(row).getByTestId("pipeline-variable-type"), { target: { value: "string" } });
    expect(pipelineNow().variables.max_iter.type).toBe("string");

    fireEvent.click(within(row).getByTestId("pipeline-variable-delete"));
    expect(Object.keys(pipelineNow().variables)).toEqual(["new_var_2"]);
    expect(tabNow().dirty).toBe(true);
  });

  // FP #948 finding: typing a name keystroke by keystroke used to remount the
  // row on every change (keyed by name) — the input lost focus after one letter
  // and the row jumped to the end of the list.
  it("renames a variable keystroke by keystroke, keeping focus and its position", () => {
    patchPipeline({
      variables: {
        a: { type: "int", default: 1 },
        new_var: { type: "int", default: 0 },
        z: { type: "int", default: 2 },
      },
    });
    render(<StorePanel />);
    const nameInput = () =>
      within(screen.getAllByTestId("pipeline-variable-row")[1]).getByTestId("pipeline-variable-name") as HTMLInputElement;
    const input = nameInput();
    input.focus();
    for (const typed of ["e", "en", "env"]) fireEvent.change(nameInput(), { target: { value: typed } });
    expect(nameInput()).toBe(input);
    expect(document.activeElement).toBe(input);
    expect(Object.keys(pipelineNow().variables)).toEqual(["a", "env", "z"]);
    expect(pipelineNow().variables.env).toEqual({ type: "int", default: 0 });
  });

  it("never commits an empty or already-taken name, and reverts it on blur", () => {
    patchPipeline({
      variables: {
        a: { type: "int", default: 1 },
        b: { type: "string", default: "x" },
      },
    });
    render(<StorePanel />);
    const nameInput = () =>
      within(screen.getAllByTestId("pipeline-variable-row")[1]).getByTestId("pipeline-variable-name") as HTMLInputElement;

    fireEvent.change(nameInput(), { target: { value: "a" } });
    expect(nameInput().value).toBe("a");
    expect(nameInput()).toHaveAttribute("aria-invalid", "true");
    expect(pipelineNow().variables).toEqual({ a: { type: "int", default: 1 }, b: { type: "string", default: "x" } });

    fireEvent.change(nameInput(), { target: { value: "" } });
    expect(Object.keys(pipelineNow().variables)).toEqual(["a", "b"]);

    fireEvent.blur(nameInput());
    expect(nameInput().value).toBe("b");
    expect(nameInput()).toHaveAttribute("aria-invalid", "false");
  });

  it("does not render the lint banner — pipeline diagnostics live on the canvas overlay (#63)", () => {
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) => ({
        ...t,
        diagnostics: ["node 'reviewer' receives edges from 2 isolated nodes without a Merge"],
      })),
    }));
    render(<StorePanel />);
    expect(screen.getByTestId("pipeline-meta")).toBeInTheDocument();
    expect(screen.queryByTestId("lint-banner")).not.toBeInTheDocument();
  });
});

describe("Pipeline info — wiring grid size on a template (#877)", () => {
  it("follows the global default until the pipeline picks a size", () => {
    render(<StorePanel />);
    expect(screen.getByTestId("pipeline-grid-size-global")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("pipeline-grid-size-global")).toHaveTextContent("Global (M)");
    expect(screen.getByTestId("pipeline-grid-size-hint")).toHaveTextContent("M · 30px");
    expect(serializePipeline(pipelineNow())).not.toContain("grid_size");
  });

  it("stores a picked size in the pipeline file, and it wins over the global default", () => {
    useWiringStore.setState({ defaultGridSize: "L" });
    render(<StorePanel />);
    fireEvent.click(screen.getByTestId("pipeline-grid-size-S"));
    expect(pipelineNow().grid_size).toBe("S");
    expect(serializePipeline(pipelineNow())).toContain("grid_size: S");
    expect(screen.getByTestId("pipeline-grid-size-S")).toHaveAttribute("aria-checked", "true");
    expect(screen.getByTestId("pipeline-grid-size-hint")).toHaveTextContent("S · 20px");
  });

  it("« Global » drops the pipeline's own size from the document", () => {
    render(<StorePanel />);
    fireEvent.click(screen.getByTestId("pipeline-grid-size-L"));
    expect(pipelineNow().grid_size).toBe("L");
    fireEvent.click(screen.getByTestId("pipeline-grid-size-global"));
    expect("grid_size" in pipelineNow()).toBe(false);
    expect(serializePipeline(pipelineNow())).not.toContain("grid_size");
  });

  it("changing the size never touches a stored waypoint", () => {
    const waypoints = [{ x: 80, y: 240 }, { x: 480, y: 240 }];
    useEditStore.setState((s) => ({
      openTabs: s.openTabs.map((t) => ({
        ...t,
        pipeline: {
          ...t.pipeline,
          edges: t.pipeline.edges.map((e) => ({ ...e, mode: "manual" as const, waypoints })),
        },
      })),
    }));
    render(<StorePanel />);
    for (const id of ["S", "M", "L", "global"]) {
      fireEvent.click(screen.getByTestId(`pipeline-grid-size-${id}`));
      expect(pipelineNow().edges[0].waypoints).toEqual(waypoints);
    }
  });
});

describe("Pipeline info — Info tab on a Run is read-only (#948)", () => {
  it.each([["running"], ["archived"]] as const)(
    "shows version, Prompt required, variables and grid size as text on a %s Run, with no input",
    (status) => {
      patchPipeline({
        version: "3.1",
        prompt_required: false,
        grid_size: "S",
        variables: { max_iter: { type: "int", default: 3 } },
      });
      render(<StorePanel run={makeRun({ status })} />);

      const meta = screen.getByTestId("pipeline-meta");
      expect(meta).toHaveAttribute("data-readonly", "true");
      expect(screen.getByTestId("pipeline-meta-version")).toHaveTextContent("3.1");
      expect(screen.getByTestId("pipeline-meta-prompt-required")).toHaveTextContent("No");
      expect(screen.getByTestId("pipeline-meta-grid-size")).toHaveTextContent("S");
      expect(screen.getByTestId("info-panel-variables")).toHaveTextContent("max_iter");
      expect(screen.getByTestId("info-panel-variables")).toHaveTextContent("3");

      expect(within(meta).queryByRole("textbox")).not.toBeInTheDocument();
      expect(within(meta).queryByRole("checkbox")).not.toBeInTheDocument();
      expect(within(meta).queryByRole("combobox")).not.toBeInTheDocument();
      expect(within(meta).queryByRole("radiogroup")).not.toBeInTheDocument();
      expect(within(meta).queryByRole("button")).not.toBeInTheDocument();
      expect(screen.queryByText("Description")).not.toBeInTheDocument();
    },
  );

  it("names the global default when the Run's pipeline carries no grid size", () => {
    render(<StorePanel run={makeRun()} />);
    expect(screen.getByTestId("pipeline-meta-grid-size")).toHaveTextContent("Global (M)");
    expect(screen.getByTestId("pipeline-meta-prompt-required")).toHaveTextContent("Yes");
  });

  it("keeps the Run stats alongside the graph stats", () => {
    render(<StorePanel run={makeRun()} />);
    expect(screen.getByTestId("run-stats")).toBeInTheDocument();
    expect(screen.getByTestId("pipeline-graph-stats")).toHaveTextContent("2 nodes");
  });
});

describe("Pipeline info — YAML tab reflects an unsaved edit (#948)", () => {
  it("shows the edit buffer while the tab is dirty, and the saved document once it is clean", async () => {
    vi.mocked(fetchPipelineDocument).mockResolvedValue("pdo_pipeline: 1\npipeline:\n  name: My Pipeline\n");
    render(<StorePanel assistantId="p1" />);

    fireEvent.click(screen.getByTestId("prompt-required-checkbox"));
    fireEvent.click(screen.getByTestId("info-tab-yaml"));
    const yaml = screen.getByTestId("info-yaml-content");
    expect(yaml).toHaveTextContent("prompt_required: false");
    expect(screen.getByTestId("yaml-unsaved-note")).toBeInTheDocument();
    expect(fetchPipelineDocument).not.toHaveBeenCalled();

    // Save lands: the tab is clean again → the saved portable document is back.
    useEditStore.setState((s) => ({ openTabs: s.openTabs.map((t) => ({ ...t, dirty: false })) }));
    await waitFor(() => expect(screen.getByTestId("info-yaml-content")).toHaveTextContent("pdo_pipeline: 1"));
    expect(fetchPipelineDocument).toHaveBeenCalledWith("p1");
    expect(screen.queryByTestId("yaml-unsaved-note")).not.toBeInTheDocument();
  });
});
