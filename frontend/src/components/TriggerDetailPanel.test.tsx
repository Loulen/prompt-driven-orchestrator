import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import TriggerDetailPanel from "./TriggerDetailPanel";
import type { Trigger, TriggerFire } from "../types";

const fetchTriggerFires = vi.fn();

vi.mock("../api", () => ({
  fetchTriggerFires: (id: string) => fetchTriggerFires(id),
}));

function trigger(overrides: Partial<Trigger> = {}): Trigger {
  return {
    id: "trg-1",
    name: "Nightly audit",
    pipeline_id: "auditor",
    pipeline_name: "Auditor",
    target_repo: "/repos/foo",
    source_branch: "main",
    input_template: "audit the codebase",
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

function fire(overrides: Partial<TriggerFire> = {}): TriggerFire {
  return {
    id: 1,
    trigger_id: "trg-1",
    ts: "2026-06-06T09:00:00.000Z",
    outcome: "fired",
    reason: null,
    run_id: "20260606-090000-abc1234",
    ...overrides,
  };
}

const noop = () => {};

describe("TriggerDetailPanel", () => {
  beforeEach(() => {
    fetchTriggerFires.mockReset();
    fetchTriggerFires.mockResolvedValue([]);
  });

  it("shows the trigger's configuration without entering edit mode", async () => {
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);
    expect(screen.getByText("Nightly audit")).toBeInTheDocument();
    expect(screen.getByText("Auditor")).toBeInTheDocument();
    // Human schedule, repo basename, input template, overlap policy.
    expect(screen.getByText("daily at 09:00")).toBeInTheDocument();
    expect(screen.getByText("audit the codebase")).toBeInTheDocument();
    expect(screen.getByText(/skip/i)).toBeInTheDocument();
    await waitFor(() => expect(fetchTriggerFires).toHaveBeenCalledWith("trg-1"));
  });

  it("renders a fired history entry with its timestamp and a link to the run", async () => {
    fetchTriggerFires.mockResolvedValue([fire()]);
    const onSelectRun = vi.fn();
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={onSelectRun} />);

    const link = await screen.findByTestId("fire-run-link");
    expect(link).toBeInTheDocument();
    link.click();
    expect(onSelectRun).toHaveBeenCalledWith("20260606-090000-abc1234");
  });

  it("renders a skipped-overlap entry with its reason and no run link", async () => {
    fetchTriggerFires.mockResolvedValue([
      fire({
        id: 2,
        outcome: "skipped-overlap",
        reason: "previous run still active",
        run_id: null,
      }),
    ]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);

    expect(await screen.findByText(/skipped-overlap/i)).toBeInTheDocument();
    expect(screen.getByText("previous run still active")).toBeInTheDocument();
    expect(screen.queryByTestId("fire-run-link")).not.toBeInTheDocument();
  });

  it("renders guard-failed and guard-error entries", async () => {
    fetchTriggerFires.mockResolvedValue([
      fire({ id: 3, outcome: "guard-error", reason: "guard timed out", run_id: null }),
      fire({ id: 2, outcome: "guard-exit-nonzero", reason: "no work to do", run_id: null }),
    ]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);

    expect(await screen.findByText(/guard-error/i)).toBeInTheDocument();
    expect(screen.getByText("guard timed out")).toBeInTheDocument();
    expect(screen.getByText(/guard-exit-nonzero/i)).toBeInTheDocument();
    expect(screen.getByText("no work to do")).toBeInTheDocument();
  });

  it("shows an empty fire-history state when the trigger never fired", async () => {
    fetchTriggerFires.mockResolvedValue([]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);
    expect(await screen.findByText(/no fires yet/i)).toBeInTheDocument();
  });
});
