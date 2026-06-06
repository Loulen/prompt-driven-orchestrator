import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import TriggersListPanel from "./TriggersListPanel";
import type { Trigger } from "../types";

const updateTrigger = vi.fn();
const deleteTrigger = vi.fn();

vi.mock("../api", () => ({
  deleteTrigger: (id: string) => deleteTrigger(id),
  updateTrigger: (id: string, req: unknown) => updateTrigger(id, req),
}));

const noop = () => {};

function trigger(overrides: Partial<Trigger> = {}): Trigger {
  return {
    id: "trg-1",
    name: "Nightly audit",
    pipeline_id: "auditor",
    pipeline_name: "Auditor",
    target_repo: "/repos/foo",
    source_branch: "main",
    input_template: "audit",
    variables: {},
    cron: "0 9 * * *",
    guard_command: null,
    overlap_policy: "skip",
    enabled: true,
    next_fire_at: "2026-06-07T09:00:00.000Z",
    last_fired_at: null,
    last_outcome: null,
    ...overrides,
  };
}

function renderPanel(
  triggers: Trigger[] = [],
  overrides: Partial<React.ComponentProps<typeof TriggersListPanel>> = {},
) {
  return render(
    <TriggersListPanel
      triggers={triggers}
      selectedTriggerId={null}
      onSelectTrigger={noop}
      onNewTrigger={noop}
      onTriggersChanged={noop}
      onRunNow={noop}
      onEditTrigger={noop}
      {...overrides}
    />,
  );
}

describe("TriggersListPanel", () => {
  beforeEach(() => {
    updateTrigger.mockReset();
    updateTrigger.mockResolvedValue(undefined);
    deleteTrigger.mockReset();
    deleteTrigger.mockResolvedValue(undefined);
  });

  it("shows an inviting empty state with an entry point when there are no triggers", () => {
    renderPanel([]);
    expect(screen.getByText(/no triggers yet/i)).toBeInTheDocument();
    // Header + empty-state call-to-action both offer "New Trigger".
    expect(screen.getAllByRole("button", { name: /new trigger/i }).length).toBeGreaterThanOrEqual(1);
  });

  it("renders a trigger row with name, pipeline and a human schedule", () => {
    renderPanel([trigger()]);
    expect(screen.getByText("Nightly audit")).toBeInTheDocument();
    expect(screen.getByText("Auditor")).toBeInTheDocument();
    expect(screen.getByText("daily at 09:00")).toBeInTheDocument();
  });

  it("renders a status dot reflecting the last outcome", () => {
    renderPanel([trigger({ last_outcome: "fired" })]);
    expect(document.querySelector(".bg-st-done")).toBeInTheDocument();
  });

  it("shows an error dot when the last outcome was an error", () => {
    renderPanel([trigger({ last_outcome: "error" })]);
    expect(document.querySelector(".bg-st-failed")).toBeInTheDocument();
  });

  it("renders a disabled trigger visibly inactive (greyed)", () => {
    const { container } = renderPanel([trigger({ enabled: false })]);
    expect(container.querySelector(".opacity-60")).toBeInTheDocument();
  });

  it("toggles a trigger off via the enable/disable toggle without selecting it", () => {
    const onSelectTrigger = vi.fn();
    renderPanel([trigger({ enabled: true })], { onSelectTrigger });
    const toggle = screen.getByTestId("trigger-toggle");
    fireEvent.click(toggle);
    expect(updateTrigger).toHaveBeenCalledWith("trg-1", { enabled: false });
    // Toggling must not also select the row (event does not bubble).
    expect(onSelectTrigger).not.toHaveBeenCalled();
  });

  it("toggles a disabled trigger back on", () => {
    renderPanel([trigger({ enabled: false })]);
    fireEvent.click(screen.getByTestId("trigger-toggle"));
    expect(updateTrigger).toHaveBeenCalledWith("trg-1", { enabled: true });
  });

  it("exposes run-now, edit and delete hover actions on a row", () => {
    const onRunNow = vi.fn();
    const onEditTrigger = vi.fn();
    const t = trigger();
    renderPanel([t], { onRunNow, onEditTrigger });
    const row = screen.getByTestId("trigger-row");

    fireEvent.click(within(row).getByTestId("trigger-run-now"));
    expect(onRunNow).toHaveBeenCalledWith(t);

    fireEvent.click(within(row).getByTestId("trigger-edit"));
    expect(onEditTrigger).toHaveBeenCalledWith(t);

    fireEvent.click(within(row).getByTestId("trigger-delete"));
    expect(deleteTrigger).toHaveBeenCalledWith("trg-1");
  });

  it("shows a status tooltip with the last run date and result on the dot", () => {
    renderPanel([
      trigger({ last_outcome: "fired", last_fired_at: "2026-06-06T09:00:00.000Z" }),
    ]);
    const dot = screen.getByTestId("trigger-status-dot");
    expect(dot).toHaveAttribute("title", expect.stringContaining("fired"));
  });
});
