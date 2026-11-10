/**
 * A Run's Info tab with a given Apparence (CONTEXT.md « Élément masquable »):
 * a field or a section hidden is not drawn; the name, the status and the alert
 * banners always are; a section whose every element is hidden goes; an
 * extension's `run/info` section hides on its own switch.
 */
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";
import type { ExtensionEntry } from "../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../shared/generated/UiContribution";
import { resetAppearance } from "../../hooks/useAppearance";
import { ELEMENTS_STORAGE_KEY } from "../../lib/appearance";
import ExtensionContextProvider from "../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../shared/extensions/store";
import type { EditorPipelineDef } from "../model/pipeline";

vi.mock("../../settings/api/settings", () => ({
  // The Source block reads the Run's drift on mount: one branch, available.
  fetchSourceDrift: vi.fn().mockResolvedValue({
    state: "available",
    source_branch: "main",
    fork: "abc1234",
    last_fetch_at: null,
    behind: 0,
    ahead: 0,
  }),
}));
vi.mock("../../run/api/runs", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  ...(await import("../../settings/api/settings")),
}));

import { InfoTab } from "./InfoTab";

const RUN: RunState = {
  collection_states: {},
  switch_states: {},
  loop_states: {},
  foreach_states: {},
  sandbox: "full",
  run_id: "20260101-100000-abcdef1",
  status: "failed",
  failure_reason: "node `build` exited 1",
  pipeline_name: "implement-ticket",
  input: null,
  harness: "claude-code",
  target_repo: "/repos/shop",
  triggered_by: "nightly",
  started_at: "2026-01-01T10:00:00.000Z",
  completed_at: "2026-01-01T10:14:02.000Z",
  nodes: {},
  edges: [],
  node_defs: [],
  sessions_spawned: 4,
  loc: { insertions: 482, deletions: 37, files_changed: 9 },
  cost: { uncosted_harnesses: [], by_harness: [], usd: 2.41, partial: false, unpriced_models: [] },
} as unknown as RunState;

const PIPELINE: EditorPipelineDef = { name: "implement-ticket", version: "1.0", variables: {}, nodes: [], edges: [] };

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
} as unknown as ExtensionEntry;

/** This browser's copy of the Apparence holds `elements`, read at the next render. */
function appearance(elements: Record<string, boolean>) {
  localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(elements));
  resetAppearance();
}

/** The daemon serves the test extension, whose `run/info` section names the Run's people. */
function withExtension() {
  setBundleLoader(async () => ({ People: () => <span data-testid="people-body">Ada</span> }));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const body = String(url) === "/ext" ? [EXTENSION] : [];
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
}

function draw(run: RunState = RUN) {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <InfoTab run={run} pipeline={PIPELINE} pipelineName={run.pipeline_name} pipelineId={null} onOpenDiff={() => {}} />
    </ExtensionContextProvider>,
  );
}

const rowKeys = () => [...document.querySelectorAll(".d-kv__row dt")].map((dt) => dt.textContent);
const sectionTitles = () => [...document.querySelectorAll(".d-sec__title")].map((title) => title.textContent);

afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
  resetAppearance();
  resetExtensions();
});

describe("a Run's Info tab, with an Apparence", () => {
  it("names the Trigger that fired the Run, its id in the tooltip (CORE-232)", async () => {
    appearance({});
    vi.stubGlobal(
      "fetch",
      vi.fn(async (url: string) =>
        String(url) === "/triggers/nightly"
          ? new Response(JSON.stringify({ id: "nightly", name: "Nightly audit" }), {
              status: 200,
              headers: { "Content-Type": "application/json" },
            })
          : new Response("[]", { status: 200, headers: { "Content-Type": "application/json" } }),
      ),
    );
    draw();
    await waitFor(() => expect(screen.getByTestId("run-trigger")).toHaveTextContent("Nightly audit"));
  });

  it("shows the defaults: no read-only note, no Status row, no Node sessions, no Graph rows", async () => {
    appearance({});
    draw();
    await screen.findByTestId("run-source-block");
    expect(screen.queryByTestId("run-info-note")).toBeNull();
    expect(screen.queryByTestId("run-status")).toBeNull();
    expect(rowKeys()).toEqual(expect.arrayContaining(["Harness", "Repo", "Sandbox", "Started", "Trigger", "Source"]));
    expect(screen.getByTestId("stat-duration")).toBeInTheDocument();
    expect(screen.queryByTestId("stat-sessions")).toBeNull();
    expect(screen.getByTestId("stat-loc")).toBeInTheDocument();
    expect(screen.getByTestId("stat-cost")).toBeInTheDocument();
    expect(screen.queryByTestId("pipeline-graph-nodes")).toBeNull();
    expect(screen.queryByTestId("pipeline-graph-edges")).toBeNull();
  });

  it("shows what the user turns on: the note, the Status row, Node sessions and the Graph rows", async () => {
    appearance({ "run.info.note": true, "run.info.status": true, "run.info.sessions": true, "run.info.graph": true });
    draw();
    await screen.findByTestId("run-source-block");
    expect(screen.getByTestId("run-info-note")).toBeInTheDocument();
    expect(screen.getByTestId("run-status")).toBeInTheDocument();
    expect(screen.getByTestId("stat-sessions")).toBeInTheDocument();
    expect(screen.getByTestId("pipeline-graph-nodes")).toHaveTextContent("0");
  });

  it("leaves out each hidden field, and keeps the name, the status icon and the failure banner", async () => {
    appearance({ "run.info.harness": false, "run.info.repo": false, "run.info.source": false, "run.info.cost": false });
    draw();
    await waitFor(() => expect(rowKeys()).toContain("Sandbox"));
    expect(rowKeys()).not.toEqual(expect.arrayContaining(["Harness"]));
    expect(rowKeys()).not.toContain("Repo");
    expect(screen.queryByTestId("run-source-block")).toBeNull();
    expect(screen.queryByTestId("stat-cost")).toBeNull();
    expect(screen.getByTestId("info-panel-name")).toHaveTextContent("implement-ticket");
    expect(screen.getByTestId("info-tab").querySelector(".pi-head svg")).not.toBeNull();
    expect(screen.getByTestId("run-failure-reason")).toHaveTextContent("node `build` exited 1");
  });

  it("removes a section whose every element is hidden: Stats, then Run", () => {
    appearance({
      "run.info.duration": false,
      "run.info.changes": false,
      "run.info.cost": false,
      "run.info.harness": false,
      "run.info.repo": false,
      "run.info.sandbox": false,
      "run.info.started": false,
      "run.info.trigger": false,
      "run.info.source": false,
    });
    draw();
    expect(screen.queryByTestId("run-stats")).toBeNull();
    expect(sectionTitles()).not.toContain("Stats");
    expect(sectionTitles()).not.toContain("Run");
    // The Pipeline section keeps its name: it never empties.
    expect(sectionTitles()).toContain("Pipeline");
    expect(screen.getByTestId("run-failure-reason")).toBeInTheDocument();
  });

  it("removes the Run section when Source is its only element shown and the Run has no drift to read", async () => {
    const { fetchSourceDrift } = await import("../../run/api/runs");
    vi.mocked(fetchSourceDrift).mockRejectedValueOnce(new Error("no drift"));
    appearance(Object.fromEntries(["harness", "repo", "sandbox", "started", "trigger"].map((k) => [`run.info.${k}`, false])));
    draw();
    await waitFor(() => expect(fetchSourceDrift).toHaveBeenCalled());
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(screen.queryByTestId("run-source-block")).toBeNull();
    expect(sectionTitles()).not.toContain("Run");
    expect(sectionTitles()).toContain("Stats");
  });

  it("keeps every alert banner whatever the Apparence", () => {
    appearance(Object.fromEntries(["note", "status", "harness", "repo", "sandbox", "started", "trigger", "source"].map((k) => [`run.info.${k}`, false])));
    draw({
      ...RUN,
      status: "running",
      failure_reason: undefined,
      awaiting_reason: "the agent asks",
      waiting_for_started_nodes: 2,
      source_fetch_error: { message: "fatal: unable to access" },
      sandbox_prep: "pending",
    } as unknown as RunState);
    expect(screen.getByTestId("run-awaiting-reason")).toBeInTheDocument();
    expect(screen.getByTestId("run-waiting-started-nodes-reason")).toBeInTheDocument();
    expect(screen.getByTestId("run-source-fetch-error")).toBeInTheDocument();
    expect(screen.getByTestId("sandbox-prep-banner")).toBeInTheDocument();
  });

  it("shows an extension's `run/info` section by default, and leaves it out once hidden", async () => {
    withExtension();
    appearance({});
    const { unmount } = draw();
    expect(await screen.findByTestId("people-body")).toBeInTheDocument();
    unmount();
    appearance({ "run.info.ext.test:people": false });
    draw();
    await screen.findByTestId("run-source-block");
    expect(screen.queryByTestId("people-body")).toBeNull();
    expect(screen.queryByTestId("extension-section-test:people")).toBeNull();
  });

  it("keeps a template's Info whole: its Graph section shows, whatever the Apparence", () => {
    appearance({ "run.info.graph": false });
    render(<InfoTab run={null} pipeline={PIPELINE} pipelineName="p" pipelineId={null} onOpenDiff={() => {}} />);
    expect(screen.getByTestId("pipeline-graph-nodes")).toBeInTheDocument();
  });
});
