/**
 * The Run canvas and Run info tab blocks of Settings › Appearance: « Compact nodes »
 * first, the master switch that greys the card's switches; a switch per element of
 * a card and per field of the Info tab, the extensions' sections by name and label;
 * beside them previews drawn with the canvas's own cards and the Info tab itself,
 * on the most recent Run or on a sample. Pointing a switch rings its element.
 */
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { RunState } from "../../../shared/generated/RunState";
import type { UiContribution } from "../../../shared/generated/UiContribution";
import type { EditorPipelineDef } from "../../../pipeline/model/pipeline";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { resetAppearance, updateAppearance, useAppearance } from "../../../hooks/useAppearance";
import type { RecentRun } from "../../store/useRecentRun";
import RunCanvasBlock from "./RunCanvasBlock";
import RunInfoBlock from "./RunInfoBlock";
import { isOn, rowOf, switchOf } from "./testing";

const node = (id: string, y: number, over: Record<string, unknown> = {}) => ({
  id,
  name: id,
  type: "agent" as const,
  interactive: false,
  inputs: [],
  outputs: [{ name: "out" }],
  view: { x: 0, y },
  ...over,
});

const PIPELINE: EditorPipelineDef = {
  name: "implement-ticket",
  version: "1.0",
  variables: {},
  nodes: [
    node("start", 0, { type: "start", outputs: [{ name: "out" }] }),
    node("plan", 100),
    node("tdd", 200),
    node("docs", 200),
    node("dispatch", 300, { orchestrator: true }),
    node("merge", 400),
  ],
  edges: [],
};

const nodeRun = (status: string, minutes: number, usd: number) => ({
  status,
  iter: 1,
  iterations: [{}],
  started_at: "2026-01-01T10:00:00Z",
  completed_at: new Date(Date.UTC(2026, 0, 1, 10, minutes)).toISOString(),
  cost: { usd, form: "reported", reported_in_usd: true, partial: false, executions: 1, readable_executions: 1 },
});

const RUN = {
  run_id: "recent",
  status: "failed",
  failure_reason: "node `merge` exited 1",
  pipeline_name: "implement-ticket",
  input: null,
  harness: "claude-code",
  sandbox: "off",
  started_at: "2026-01-01T10:00:00Z",
  completed_at: "2026-01-01T10:14:00Z",
  sessions_spawned: 5,
  nodes: {
    plan: nodeRun("completed", 3, 0.31),
    tdd: nodeRun("completed", 7, 1.24),
    docs: nodeRun("completed", 2, 0.18),
    dispatch: nodeRun("completed", 4, 0.68),
    merge: nodeRun("failed", 1, 0.05),
  },
  edges: [],
  node_defs: [{ id: "dispatch", orchestrator: true }],
  loop_states: {},
  foreach_states: {},
  collection_states: {},
  switch_states: {},
} as unknown as RunState;

const RECENT: RecentRun = { run: RUN, pipeline: PIPELINE };

const EXTENSION: ExtensionEntry = {
  id: "test",
  name: "Test",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/test/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions: [{ location: "run/info", id: "people", label: "People", type: "section", component: "People" }] as UiContribution[],
    commands: [],
    locations: [],
  },
};

function serve() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const u = String(url);
      const body =
        u === "/ext"
          ? [EXTENSION]
          : u.endsWith("/children")
            ? { run_id: "recent", nodes: [{ node_id: "dispatch", children: [{ run_id: "c1", status: "running" }, { run_id: "c2", status: "completed" }, { run_id: "c3", status: "completed" }] }] }
            : u.includes("agent-profiles")
              ? { profiles: [] }
              : {};
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
}

/** The Actor's Apparence, as the store holds it. */
function Probe() {
  const { compactNodes, elements } = useAppearance();
  return <output data-testid="probe">{JSON.stringify({ compactNodes, elements })}</output>;
}
const stored = () => JSON.parse(screen.getByTestId("probe").textContent ?? "{}");

function mount(block: "canvas" | "info", recent: RecentRun | null = RECENT, none = false) {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      {block === "canvas" ? <RunCanvasBlock recent={recent} none={none} /> : <RunInfoBlock recent={recent} none={none} />}
      <Probe />
    </ExtensionContextProvider>,
  );
}

const canvasPreview = () => screen.getByTestId("appearance-preview-run-canvas");
const infoPreview = () => screen.getByTestId("appearance-preview-run-info");
const cardOf = (name: string) =>
  within(canvasPreview())
    .getAllByTestId("node-card")
    .find((card) => card.querySelector(".cv-n__name")?.textContent === name)!;
const cellsOf = (name: string) => [...cardOf(name).querySelectorAll(".cv-n__cell")].map((cell) => cell.getAttribute("title")?.split(":")[0]);

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  resetExtensions();
  setBundleLoader(async () => ({ People: () => <span data-testid="people-body">Ada</span> }));
  serve();
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the Run canvas block", () => {
  it("leads with Compact nodes, then a switch per element of a card at its default", () => {
    mount("canvas");
    const block = screen.getByTestId("appearance-block-run-canvas");
    expect(block).toHaveTextContent("Always shown: Type icon, name, status, child runs of an orchestrator");
    const labels = [...block.querySelectorAll(".set-row__name")].map((name) => name.textContent);
    expect(labels).toEqual(["Compact nodes", "Model", "Iteration", "Elapsed time", "Cost", "Isolation flag", "Loop badge", "Agent profile marker"]);
    expect(isOn(switchOf("compact-nodes"))).toBe(false);
    expect(isOn(switchOf("run.canvas.agent"))).toBe(false);
    expect(isOn(switchOf("run.canvas.iteration"))).toBe(false);
    expect(isOn(switchOf("run.canvas.elapsed"))).toBe(true);
    expect(isOn(switchOf("run.canvas.cost"))).toBe(true);
    expect(isOn(switchOf("run.canvas.isolation"))).toBe(false);
    expect(isOn(switchOf("run.canvas.loop"))).toBe(true);
    expect(isOn(switchOf("run.canvas.profile"))).toBe(true);
  });

  it("greys the card's switches while Compact nodes is on, and says why", () => {
    mount("canvas");
    fireEvent.click(switchOf("compact-nodes"));
    expect(stored().compactNodes).toBe(true);
    for (const id of ["agent", "iteration", "elapsed", "cost", "isolation", "loop", "profile"]) expect(switchOf(`run.canvas.${id}`)).toBeDisabled();
    expect(switchOf("compact-nodes")).toBeEnabled();
    expect(screen.getByText("Compact nodes is on: cards show their icon, name and status only.")).toBeInTheDocument();
    // Greyed, they keep their value.
    expect(isOn(switchOf("run.canvas.elapsed"))).toBe(true);
    fireEvent.click(switchOf("compact-nodes"));
    expect(switchOf("run.canvas.elapsed")).toBeEnabled();
  });

  it("previews a few cards of the most recent Run with a fork, its orchestrator's child runs and its caption", async () => {
    mount("canvas");
    const preview = canvasPreview();
    expect(preview).toHaveTextContent("Your most recent run · implement-ticket");
    expect(preview.closest("[inert]") ?? preview.querySelector("[inert]")).not.toBeNull();
    const rows = [...preview.querySelectorAll(".set-preview__row")].map((row) =>
      [...row.querySelectorAll(".cv-n__name")].map((name) => name.textContent),
    );
    expect(rows).toEqual([["plan"], ["tdd", "docs"], ["dispatch"], ["merge"]]);
    expect(preview.querySelectorAll(".set-preview__wires")).toHaveLength(3);
    await waitFor(() => expect(within(cardOf("dispatch")).getByTestId("node-child-runs")).toHaveTextContent("3 runs"));
  });

  it("draws the preview's cards as the Apparence says: shorter as cells hide, one line with Compact nodes", () => {
    mount("canvas");
    expect(cellsOf("plan")).toEqual(["Elapsed", "Cost"]);
    fireEvent.click(switchOf("run.canvas.elapsed"));
    fireEvent.click(switchOf("run.canvas.cost"));
    expect(stored().elements).toEqual({ "run.canvas.elapsed": false, "run.canvas.cost": false });
    expect(cardOf("plan").querySelector(".cv-n__grid")).toBeNull();
    fireEvent.click(switchOf("run.canvas.agent"));
    expect(cellsOf("plan")).toEqual(["Inherits the run agent"]);
    fireEvent.click(switchOf("compact-nodes"));
    expect(cardOf("plan")).toHaveAttribute("data-compact", "true");
    expect(cardOf("plan").querySelector(".cv-n__grid")).toBeNull();
  });

  it("rings the pointed element, and brings a hidden one back as a ghost", () => {
    mount("canvas");
    fireEvent.pointerEnter(rowOf("run.canvas.cost").parentElement!);
    const cost = cardOf("plan").querySelector('[data-element="run.canvas.cost"]')!;
    expect(cost).toHaveAttribute("data-pointed");
    expect(cost).not.toHaveAttribute("data-ghost");
    fireEvent.pointerLeave(rowOf("run.canvas.cost").parentElement!);
    fireEvent.focus(switchOf("run.canvas.iteration"));
    const iteration = cardOf("plan").querySelector('[data-element="run.canvas.iteration"]')!;
    expect(iteration).toHaveAttribute("data-ghost");
    expect(iteration).toHaveAttribute("data-pointed");
  });

  it("points nothing out while Compact nodes is on", () => {
    updateAppearance((a) => ({ ...a, compactNodes: true }));
    mount("canvas");
    fireEvent.pointerEnter(rowOf("run.canvas.cost").parentElement!);
    expect(canvasPreview().querySelector("[data-pointed]")).toBeNull();
  });

  it("previews a sample Run when the user sees none", () => {
    mount("canvas", null, true);
    expect(canvasPreview()).toHaveAttribute("data-sample", "true");
    expect(canvasPreview()).toHaveTextContent("Sample run · you see no run yet");
    expect(within(canvasPreview()).getAllByTestId("node-card")).toHaveLength(5);
    expect(within(cardOf("agentic tests")).getByTestId("node-child-runs")).toHaveTextContent("3 runs");
  });
});

describe("the Run info tab block", () => {
  it("lists its groups and fields at their defaults, then the extension sections by name and label", async () => {
    mount("info");
    const block = screen.getByTestId("appearance-block-run-info");
    expect(block).toHaveTextContent("Always shown: Name, status, alert banners");
    await screen.findByTestId("setting-row-run.info.ext.test:people");
    const groups = [...block.querySelectorAll(".set-ablock__group")].map((group) => group.textContent);
    expect(groups).toEqual(["Header", "Run", "Stats", "Pipeline", "Extension sections"]);
    expect(within(rowOf("run.info.ext.test:people")).getByText("Test · People")).toBeInTheDocument();
    for (const id of ["note", "status", "sessions", "graph"]) expect(isOn(switchOf(`run.info.${id}`))).toBe(false);
    for (const id of ["harness", "repo", "sandbox", "started", "trigger", "source", "duration", "changes", "cost"]) {
      expect(isOn(switchOf(`run.info.${id}`))).toBe(true);
    }
    expect(isOn(switchOf("run.info.ext.test:people"))).toBe(true);
  });

  it("previews the Info tab of the most recent Run, and follows each switch", async () => {
    mount("info");
    const preview = infoPreview();
    expect(preview).toHaveTextContent("Your most recent run · implement-ticket");
    expect(await within(preview).findByTestId("people-body")).toBeInTheDocument();
    expect(within(preview).getByTestId("stat-duration")).toBeInTheDocument();
    expect(within(preview).getByTestId("run-failure-reason")).toBeInTheDocument();
    for (const id of ["duration", "changes", "cost"]) fireEvent.click(switchOf(`run.info.${id}`));
    fireEvent.click(switchOf("run.info.ext.test:people"));
    fireEvent.pointerLeave(rowOf("run.info.ext.test:people").parentElement!);
    expect(within(preview).queryByTestId("run-stats")).toBeNull();
    expect(within(preview).queryByTestId("people-body")).toBeNull();
    // The failure reason is an alert: it stays.
    expect(within(preview).getByTestId("run-failure-reason")).toBeInTheDocument();
    expect(stored().elements).toMatchObject({ "run.info.duration": false, "run.info.ext.test:people": false });
  });

  it("brings a hidden field back as a ghost while its switch is pointed", () => {
    mount("info");
    fireEvent.pointerEnter(rowOf("run.info.sessions").parentElement!);
    expect(within(infoPreview()).getByTestId("stat-sessions")).toHaveAttribute("data-ghost");
    fireEvent.pointerLeave(rowOf("run.info.sessions").parentElement!);
    expect(within(infoPreview()).queryByTestId("stat-sessions")).toBeNull();
  });

  it("previews a sample Run without extension sections when the user sees none", async () => {
    mount("info", null, true);
    await screen.findByTestId("setting-row-run.info.ext.test:people");
    expect(infoPreview()).toHaveAttribute("data-sample", "true");
    expect(infoPreview()).toHaveTextContent("Sample run · you see no run yet");
    expect(within(infoPreview()).queryByTestId("people-body")).toBeNull();
    expect(within(infoPreview()).getByTestId("stat-cost")).toBeInTheDocument();
  });
});
