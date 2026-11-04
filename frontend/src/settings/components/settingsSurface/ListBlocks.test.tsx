/**
 * The Triggers list and Pipelines list blocks of Settings › Appearance: a switch per
 * element of a row at its default, the extensions' contributions under *Extensions*,
 * and beside them a preview made of the list's own rows, on the user's most recent
 * Triggers and Pipelines or on a sample. Pointing a switch rings its element.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import type { TriggerListEntry } from "../../../shared/generated/TriggerListEntry";
import type { UiContribution } from "../../../shared/generated/UiContribution";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { resetAppearance, useAppearance } from "../../../hooks/useAppearance";
import { useEditStore } from "../../../pipeline/store/editStore";
import PipelinesListBlock from "./PipelinesListBlock";
import TriggersListBlock from "./TriggersListBlock";
import { isOn, rowOf, switchOf } from "./testing";

const HOUR = 3_600_000;

const trigger = (id: string, name: string, over: Partial<TriggerListEntry> = {}) =>
  ({
    id,
    name,
    pipeline_id: "p",
    pipeline_name: `${id}-pipeline`,
    effective_repo: "",
    input_template: "",
    variables: {},
    cron: "0 3 * * *",
    overlap_policy: "skip",
    auto_name: false,
    enabled: true,
    next_fire_at: new Date(Date.now() + 7 * HOUR).toISOString(),
    ...over,
  }) as TriggerListEntry;

const TRIGGERS = [
  trigger("old", "Weekly bump", { last_fired_at: "2026-01-01T00:00:00Z", last_outcome: "fired" }),
  trigger("recent", "Nightly review", {
    last_fired_at: "2026-01-02T00:00:00Z",
    last_outcome: "fired",
    guard_command: "test -f ready",
  }),
];

const pipeline = (id: string, modified: string, over: Partial<PipelineListEntry> = {}) =>
  ({
    id,
    name: id,
    scope: "instance",
    path: "",
    node_count: 2,
    modified,
    variables: {},
    prompt_required: false,
    validation: { kind: "all" },
    version: { number: 6, title: "" },
    ...over,
  }) as PipelineListEntry;

const PIPELINES = [
  pipeline("grill", "2026-01-01T00:00:00Z"),
  pipeline("implement", "2026-01-02T00:00:00Z", {
    version: { number: 14, title: "" },
    draft: { changes: 3, nodes: 2, participants: [] },
  }),
];

const EXTENSION: ExtensionEntry = {
  id: "test",
  name: "Test",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/test/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions: [
      { location: "trigger/item", id: "owner", label: "Owner", type: "decoration", component: "Owner" },
      { location: "pipeline/item", id: "tag", label: "Tag", type: "decoration", component: "Tag" },
    ] as UiContribution[],
    commands: [],
    locations: [],
  },
};

function Probe() {
  return <output data-testid="probe">{JSON.stringify(useAppearance().elements)}</output>;
}
const deviations = () => JSON.parse(screen.getByTestId("probe").textContent ?? "{}");

function mount(triggers: TriggerListEntry[] = TRIGGERS, pipelines: PipelineListEntry[] = PIPELINES) {
  useEditStore.setState({ pipelines });
  return render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <TriggersListBlock triggers={triggers} />
      <PipelinesListBlock projects={[]} />
      <Probe />
    </ExtensionContextProvider>,
  );
}

const triggersPreview = () => screen.getByTestId("appearance-preview-triggers-list");
const pipelinesPreview = () => screen.getByTestId("appearance-preview-pipelines-list");
const triggerRow = (id: string) => triggersPreview().querySelector(`[data-trigger-row="${id}"]`) as HTMLElement;
const pipelineRow = (id: string) => within(pipelinesPreview()).getByTestId(`preview-library-row-${id}`);
const point = (id: string) => fireEvent.pointerEnter(rowOf(id).parentElement!);

let pipelinesBefore: PipelineListEntry[];

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  resetExtensions();
  pipelinesBefore = useEditStore.getState().pipelines;
  setBundleLoader(async () => ({
    Owner: () => <span data-testid="owner">AL</span>,
    Tag: () => <span data-testid="tag">T</span>,
  }));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const body = String(url) === "/ext" ? [EXTENSION] : [];
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
  useEditStore.setState({ pipelines: pipelinesBefore });
});

describe("the Triggers list block", () => {
  it("says what it always shows, its switches at their defaults, then the extensions'", async () => {
    mount();
    const block = screen.getByTestId("appearance-block-triggers-list");
    expect(block).toHaveTextContent("Triggers list");
    expect(block).toHaveTextContent("Always shown: Name, enable switch, last result");
    const on = (id: string) => isOn(switchOf(`triggers.list.${id}`));
    expect(["pipeline", "last", "next", "guard", "overlap"].map(on)).toEqual([true, false, true, true, false]);
    expect(await screen.findByTestId("setting-row-triggers.list.ext.test:owner")).toHaveTextContent("Test · Owner");
    expect(isOn(switchOf("triggers.list.ext.test:owner"))).toBe(true);
    expect(within(block).getByText("Extensions")).toBeInTheDocument();
  });

  it("previews the Trigger that fired last and the next one, as the list draws them, inert", async () => {
    mount();
    expect(triggersPreview()).toHaveTextContent("Your triggers");
    const rows = triggersPreview().querySelectorAll("[data-trigger-row]");
    expect([...rows].map((r) => r.getAttribute("data-trigger-row"))).toEqual(["recent", "old"]);
    const recent = within(triggerRow("recent"));
    expect(recent.getByTestId("trigger-guard")).toBeInTheDocument();
    expect(await recent.findByTestId("owner")).toBeInTheDocument();
    expect(recent.queryByTestId("trigger-row-actions")).toBeNull();
    expect(triggerRow("recent")).toHaveAttribute("tabindex", "-1");
  });

  it("follows each switch at once, keeping only the deviations", () => {
    mount();
    for (const id of ["next", "overlap", "guard"]) fireEvent.click(switchOf(`triggers.list.${id}`));
    expect(deviations()).toEqual({
      "triggers.list.next": false,
      "triggers.list.overlap": true,
      "triggers.list.guard": false,
    });
    const recent = within(triggerRow("recent"));
    expect(recent.queryByTestId("trigger-schedule")).toBeNull();
    expect(recent.queryByTestId("trigger-guard")).toBeNull();
    expect(recent.getByTestId("trigger-overlap")).toBeInTheDocument();
    expect(screen.getByTestId("setting-row-triggers.list.overlap-changed")).toHaveAttribute("title", "Default: Hidden");
  });

  it("hides an extension's contribution in the preview", async () => {
    mount();
    await within(triggerRow("recent")).findByTestId("owner");
    fireEvent.click(await screen.findByRole("switch", { name: "Test · Owner" }));
    expect(deviations()).toEqual({ "triggers.list.ext.test:owner": false });
    expect(within(triggerRow("recent")).queryByTestId("owner")).toBeNull();
  });

  it("rings the element a switch points to, and brings a hidden one back as a ghost", () => {
    mount();
    point("triggers.list.guard");
    const guard = within(triggerRow("recent")).getByTestId("trigger-guard");
    expect(guard).toHaveAttribute("data-pointed");
    expect(guard).not.toHaveAttribute("data-ghost");
    point("triggers.list.last");
    const last = within(triggerRow("recent")).getByTestId("trigger-last-fired");
    expect(last).toHaveAttribute("data-pointed");
    expect(last).toHaveAttribute("data-ghost");
  });

  it("drops the preview to one line once the second line is hidden, a ghost line while one is pointed", () => {
    mount();
    fireEvent.click(switchOf("triggers.list.pipeline"));
    fireEvent.click(switchOf("triggers.list.next"));
    expect(triggerRow("recent")).toHaveAttribute("data-lines", "1");
    point("triggers.list.next");
    expect(triggerRow("recent")).toHaveAttribute("data-ghost-line");
    expect(within(triggerRow("recent")).getByTestId("trigger-schedule")).toHaveAttribute("data-ghost");
    expect(within(triggerRow("recent")).queryByTestId("trigger-pipeline")).toBeNull();
    fireEvent.pointerLeave(rowOf("triggers.list.next").parentElement!);
    expect(triggerRow("recent").querySelector(".lp-l2")).toBeNull();
  });

  it("shows a sample without extension marks when the user sees no Trigger", async () => {
    mount([]);
    expect(triggersPreview()).toHaveAttribute("data-sample");
    expect(triggersPreview()).toHaveTextContent("Sample trigger · you see no trigger yet");
    await screen.findByTestId("setting-row-triggers.list.ext.test:owner");
    await act(async () => {});
    expect(within(triggersPreview()).queryByTestId("owner")).toBeNull();
  });
});

describe("the Pipelines list block", () => {
  it("says what it always shows, its switches all on by default, then the extensions'", async () => {
    mount();
    const block = screen.getByTestId("appearance-block-pipelines-list");
    expect(block).toHaveTextContent("Pipelines list");
    expect(block).toHaveTextContent("Always shown: Name, select control");
    const on = (id: string) => isOn(switchOf(`pipelines.list.${id}`));
    expect(["version", "draft", "modified", "validation"].map(on)).toEqual([true, true, true, true]);
    expect(await screen.findByTestId("setting-row-pipelines.list.ext.test:tag")).toHaveTextContent("Test · Tag");
  });

  it("previews the Pipeline edited last and another one, as the list draws them, inert", async () => {
    mount();
    expect(pipelinesPreview()).toHaveTextContent("Your pipelines");
    const implement = within(pipelineRow("implement"));
    expect(implement.getByTestId("library-row-version")).toHaveTextContent("v14");
    expect(implement.getByTestId("library-row-draft")).toHaveTextContent("draft: 3 changes");
    expect(implement.getByTestId("library-row-validation")).toHaveTextContent("All");
    expect(await implement.findByTestId("tag")).toBeInTheDocument();
    expect(implement.queryByTestId("library-row-actions")).toBeNull();
    expect(pipelineRow("implement")).toHaveAttribute("tabindex", "-1");
  });

  it("hides the version and the validation, the draft marker stays", () => {
    mount();
    fireEvent.click(switchOf("pipelines.list.version"));
    fireEvent.click(switchOf("pipelines.list.validation"));
    expect(deviations()).toEqual({ "pipelines.list.version": false, "pipelines.list.validation": false });
    const implement = within(pipelineRow("implement"));
    expect(implement.queryByTestId("library-row-version")).toBeNull();
    expect(implement.queryByTestId("library-row-validation")).toBeNull();
    expect(implement.getByTestId("library-row-draft")).toBeInTheDocument();
  });

  it("drops the preview to one line once the second line and its contributions are hidden", async () => {
    mount();
    await within(pipelineRow("implement")).findByTestId("tag");
    for (const id of ["version", "draft", "modified", "ext.test:tag"]) fireEvent.click(switchOf(`pipelines.list.${id}`));
    expect(pipelineRow("implement")).toHaveAttribute("data-lines", "1");
    expect(pipelineRow("grill")).toHaveAttribute("data-lines", "1");
  });

  it("shows a sample without extension marks when the user sees no Pipeline", async () => {
    mount(TRIGGERS, []);
    expect(pipelinesPreview()).toHaveAttribute("data-sample");
    expect(pipelinesPreview()).toHaveTextContent("Sample pipeline · you see no pipeline yet");
    await screen.findByTestId("setting-row-pipelines.list.ext.test:tag");
    await act(async () => {});
    expect(within(pipelinesPreview()).queryByTestId("tag")).toBeNull();
  });
});
