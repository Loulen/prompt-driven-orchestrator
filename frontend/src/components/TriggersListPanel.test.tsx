import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import TriggersListPanel from "./TriggersListPanel";
import type { Trigger } from "../types";

vi.mock("../api", () => ({
  deleteTrigger: vi.fn().mockResolvedValue(undefined),
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

function renderPanel(triggers: Trigger[] = []) {
  return render(
    <TriggersListPanel
      triggers={triggers}
      selectedTriggerId={null}
      onSelectTrigger={noop}
      onNewTrigger={noop}
      onTriggersChanged={noop}
    />,
  );
}

describe("TriggersListPanel", () => {
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
});
