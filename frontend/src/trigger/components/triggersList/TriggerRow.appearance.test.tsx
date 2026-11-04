/**
 * A Trigger's row with a given Apparence (CONTEXT.md « Élément masquable »): an
 * element hidden is not drawn, its name, its enable switch and its last result
 * always are; the row takes one line when its whole second line is hidden, the
 * last result beside the switch; enable, select and run now work the same.
 */
import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../../shared/generated/ExtensionEntry";
import type { Trigger } from "../../../shared/generated/Trigger";
import type { UiContribution } from "../../../shared/generated/UiContribution";
import ExtensionContextProvider from "../../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../../shared/extensions/store";
import { resetAppearance } from "../../../hooks/useAppearance";
import { ELEMENTS_STORAGE_KEY } from "../../../lib/appearance";
import TriggerRow from "./TriggerRow";

const HOUR = 3_600_000;

function trigger(over: Partial<Trigger> = {}): Trigger {
  return {
    id: "trg-1",
    name: "Nightly review",
    pipeline_id: "review",
    pipeline_name: "review-pipeline",
    input_template: "",
    variables: {},
    cron: "0 3 * * *",
    guard_command: "test -f ready",
    overlap_policy: "skip",
    auto_name: false,
    enabled: true,
    next_fire_at: new Date(Date.now() + 7 * HOUR).toISOString(),
    last_fired_at: new Date(Date.now() - 2 * HOUR).toISOString(),
    last_outcome: "fired",
    ...over,
  };
}

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
    ] as UiContribution[],
    commands: [],
    locations: [],
  },
};

function appearance(elements: Record<string, boolean>) {
  localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(elements));
  resetAppearance();
}

const handlers = () => ({
  onSelect: vi.fn(),
  onSelectControl: vi.fn(),
  onRunNow: vi.fn(),
  onDelete: vi.fn(),
  onToggle: vi.fn(),
});

function mount(t: Trigger = trigger(), on = handlers()) {
  render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <TriggerRow trigger={t} isSelected={false} rowSelected={false} {...on} />
    </ExtensionContextProvider>,
  );
  return on;
}

const row = () => screen.getByTestId("trigger-row");
const NO_SECOND_LINE = { "triggers.list.pipeline": false, "triggers.list.next": false };

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  resetExtensions();
  setBundleLoader(async () => ({ Owner: () => <span data-testid="owner">AL</span> }));
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
});

describe("a Trigger's row, with a given Apparence", () => {
  it("draws, by default, its pipeline, its next fire and its guard, never its last fire nor its overlap", async () => {
    mount();
    expect(screen.getByTestId("trigger-pipeline")).toHaveTextContent("review-pipeline");
    expect(screen.getByTestId("trigger-schedule")).toBeInTheDocument();
    expect(screen.getByTestId("trigger-guard")).toBeInTheDocument();
    expect(screen.queryByTestId("trigger-last-fired")).toBeNull();
    expect(screen.queryByTestId("trigger-overlap")).toBeNull();
    expect(await screen.findByTestId("owner")).toBeInTheDocument();
    expect(row()).toHaveAttribute("data-lines", "2");
  });

  it("draws every element the Apparence shows", () => {
    appearance({ "triggers.list.last": true, "triggers.list.overlap": true });
    mount();
    expect(screen.getByTestId("trigger-last-fired")).toHaveTextContent("2h");
    expect(screen.getByTestId("trigger-overlap")).toBeInTheDocument();
  });

  it.each([
    ["triggers.list.pipeline", "trigger-pipeline"],
    ["triggers.list.last", "trigger-last-fired"],
    ["triggers.list.next", "trigger-schedule"],
    ["triggers.list.guard", "trigger-guard"],
    ["triggers.list.overlap", "trigger-overlap"],
  ])("leaves %s out once hidden, and keeps the name, the switch and the last result", (id, testId) => {
    appearance({ "triggers.list.last": true, "triggers.list.overlap": true, [id]: false });
    mount();
    expect(screen.queryByTestId(testId)).toBeNull();
    expect(screen.getByTestId("trigger-name")).toHaveTextContent("Nightly review");
    expect(screen.getByTestId("trigger-toggle")).toBeInTheDocument();
    expect(screen.getByTestId("trigger-status-dot")).toHaveAttribute("data-outcome", "fired");
  });

  it("hides an extension's contribution, the others staying", async () => {
    appearance({ "triggers.list.ext.test:owner": false });
    mount();
    await act(async () => {});
    expect(screen.queryByTestId("owner")).toBeNull();
    expect(screen.getByTestId("trigger-guard")).toBeInTheDocument();
  });

  it("takes one line once its whole second line is hidden, the last result beside the switch", () => {
    appearance(NO_SECOND_LINE);
    mount();
    expect(row()).toHaveAttribute("data-lines", "1");
    expect(row().querySelector(".lp-l2")).toBeNull();
    const lead = row().querySelector(".lp-lead--trigger") as HTMLElement;
    expect(within(lead).getByTestId("trigger-toggle")).toBeInTheDocument();
    expect(within(lead).getByTestId("trigger-status-dot")).toBeInTheDocument();
  });

  it("takes one line on a Trigger that never fired too: the Apparence decides, not the Trigger", () => {
    appearance(NO_SECOND_LINE);
    mount(trigger({ last_fired_at: undefined, last_outcome: undefined }));
    expect(row()).toHaveAttribute("data-lines", "1");
  });

  it("keeps two lines while one element of the second line is shown, even one this Trigger lacks", () => {
    appearance({ ...NO_SECOND_LINE, "triggers.list.last": true });
    mount(trigger({ last_fired_at: undefined, last_outcome: undefined }));
    expect(row()).toHaveAttribute("data-lines", "2");
  });

  it("on one line, enables, selects, opens and runs now as before", () => {
    appearance(NO_SECOND_LINE);
    const on = mount();
    fireEvent.click(screen.getByTestId("trigger-toggle"));
    expect(on.onToggle).toHaveBeenCalledOnce();
    expect(on.onSelect).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Select Nightly review" }));
    expect(on.onSelectControl).toHaveBeenCalledWith(false);
    fireEvent.click(row());
    expect(on.onSelect).toHaveBeenCalledOnce();
    fireEvent.click(screen.getByTestId("trigger-run-now"));
    expect(on.onRunNow).toHaveBeenCalledOnce();
  });
});
