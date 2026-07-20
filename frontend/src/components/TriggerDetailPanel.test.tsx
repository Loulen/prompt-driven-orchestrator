import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, fireEvent } from "@testing-library/react";
import TriggerDetailPanel from "./TriggerDetailPanel";
import type { Trigger, TriggerFire } from "../types";

const fetchTriggerFires = vi.fn();
const testGuard = vi.fn();

vi.mock("../api", () => ({
  fetchTriggerFires: (id: string) => fetchTriggerFires(id),
  testGuard: (cmd: string, repo?: string) => testGuard(cmd, repo),
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
    guard_stdout: null,
    guard_stderr: null,
    guard_exit_code: null,
    ...overrides,
  };
}

const noop = () => {};

describe("TriggerDetailPanel", () => {
  beforeEach(() => {
    fetchTriggerFires.mockReset();
    fetchTriggerFires.mockResolvedValue([]);
    testGuard.mockReset();
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

  it("shows the concurrency cap for a bounded-allow trigger (#239)", () => {
    render(
      <TriggerDetailPanel
        trigger={trigger({ overlap_policy: "allow", max_concurrent: 3 })}
        onSelectRun={noop}
      />,
    );
    expect(screen.getByTestId("trigger-detail-overlap")).toHaveTextContent("max 3 concurrent");
  });

  it("shows unlimited for an allow trigger without a cap (#239)", () => {
    render(
      <TriggerDetailPanel
        trigger={trigger({ overlap_policy: "allow", max_concurrent: null })}
        onSelectRun={noop}
      />,
    );
    expect(screen.getByTestId("trigger-detail-overlap")).toHaveTextContent(/unlimited/i);
  });

  it("shows skip (default) for a skip trigger (#239 regression)", () => {
    render(
      <TriggerDetailPanel
        trigger={trigger({ overlap_policy: "skip" })}
        onSelectRun={noop}
      />,
    );
    expect(screen.getByTestId("trigger-detail-overlap")).toHaveTextContent("skip (default)");
  });

  // --- #244: guard-output disclosure on guard-exit-nonzero rows ---

  it("shows no guard-output toggle on a non-guard (fired) row", async () => {
    fetchTriggerFires.mockResolvedValue([fire()]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);
    await screen.findByTestId("fire-entry");
    expect(screen.queryByTestId("fire-guard-output-toggle")).not.toBeInTheDocument();
  });

  it("shows no guard-output toggle on a guard-error row (D2)", async () => {
    // guard-error already surfaces its detail via `reason`; no captured streams.
    fetchTriggerFires.mockResolvedValue([
      fire({ id: 5, outcome: "guard-error", reason: "guard timed out", run_id: null }),
    ]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);
    expect(await screen.findByText(/guard-error/i)).toBeInTheDocument();
    expect(screen.queryByTestId("fire-guard-output-toggle")).not.toBeInTheDocument();
  });

  it("reveals exit code + both streams behind the toggle on a guard-exit-nonzero row", async () => {
    fetchTriggerFires.mockResolvedValue([
      fire({
        id: 6,
        outcome: "guard-exit-nonzero",
        reason: "guard exited non-zero",
        run_id: null,
        guard_stdout: "checked 0 issues",
        guard_stderr: "gh: no work to do",
        guard_exit_code: 7,
      }),
    ]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);

    const toggle = await screen.findByTestId("fire-guard-output-toggle");
    // Collapsed by default: the output container is absent.
    expect(screen.queryByTestId("fire-guard-output")).not.toBeInTheDocument();

    toggle.click();

    const output = await screen.findByTestId("fire-guard-output");
    expect(output).toHaveTextContent("7");
    expect(output).toHaveTextContent("checked 0 issues");
    expect(output).toHaveTextContent("gh: no work to do");
  });

  it("omits an empty stream block (stdout empty, stderr present)", async () => {
    fetchTriggerFires.mockResolvedValue([
      fire({
        id: 7,
        outcome: "guard-exit-nonzero",
        reason: "guard exited non-zero",
        run_id: null,
        guard_stdout: "",
        guard_stderr: "gh: no work to do",
        guard_exit_code: 1,
      }),
    ]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);

    (await screen.findByTestId("fire-guard-output-toggle")).click();
    const output = await screen.findByTestId("fire-guard-output");
    expect(output).toHaveTextContent("stderr");
    expect(output).toHaveTextContent("gh: no work to do");
    // The empty stdout stream renders no labelled block.
    expect(output).not.toHaveTextContent("stdout");
  });

  it("shows the toggle when only the exit code is present (empty streams)", async () => {
    // A bare `exit 3` guard prints nothing; the exit code alone is the diagnostic.
    fetchTriggerFires.mockResolvedValue([
      fire({
        id: 8,
        outcome: "guard-exit-nonzero",
        reason: "guard exited non-zero",
        run_id: null,
        guard_stdout: "",
        guard_stderr: "",
        guard_exit_code: 3,
      }),
    ]);
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);

    (await screen.findByTestId("fire-guard-output-toggle")).click();
    const output = await screen.findByTestId("fire-guard-output");
    expect(output).toHaveTextContent("3");
    expect(output).not.toHaveTextContent("stdout");
    expect(output).not.toHaveTextContent("stderr");
  });

  // --- #351: Test guard (dry-run) from a saved trigger's detail panel ---

  const GUARD = "gh issue list --label ready-for-agent";

  it("hides the Test guard button when the trigger has no guard command", () => {
    render(<TriggerDetailPanel trigger={trigger()} onSelectRun={noop} />);
    expect(screen.queryByTestId("guard-test-button")).not.toBeInTheDocument();
  });

  it("shows the Test guard button when the trigger has a guard command", () => {
    render(<TriggerDetailPanel trigger={trigger({ guard_command: GUARD })} onSelectRun={noop} />);
    expect(screen.getByTestId("guard-test-button")).toBeInTheDocument();
  });

  it("runs the SAVED guard command + repo and shows 'Would fire' with the stdout", async () => {
    testGuard.mockResolvedValue({
      outcome: "pass",
      stdout: "issue-123\n",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    render(<TriggerDetailPanel trigger={trigger({ guard_command: GUARD })} onSelectRun={noop} />);

    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-verdict")).toHaveTextContent("Would fire");
    expect(screen.getByTestId("guard-test-output")).toHaveTextContent("issue-123");
    // The #351 twist vs #350: the stored guard_command + target_repo, not live form values.
    expect(testGuard).toHaveBeenCalledWith(GUARD, "/repos/foo");
  });

  it("shows 'Would skip' with the exit code and stderr for a non-zero guard", async () => {
    testGuard.mockResolvedValue({
      outcome: "skip",
      stdout: "",
      stderr: "no work to do",
      exit_code: 3,
      detail: null,
    });
    render(<TriggerDetailPanel trigger={trigger({ guard_command: GUARD })} onSelectRun={noop} />);

    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-verdict")).toHaveTextContent("Would skip");
    const output = screen.getByTestId("guard-test-output");
    expect(output).toHaveTextContent("3");
    expect(output).toHaveTextContent("no work to do");
  });

  it("shows 'Guard error' for an error verdict", async () => {
    testGuard.mockResolvedValue({
      outcome: "error",
      stdout: "",
      stderr: "",
      exit_code: null,
      detail: "guard timed out",
    });
    render(<TriggerDetailPanel trigger={trigger({ guard_command: GUARD })} onSelectRun={noop} />);

    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-verdict")).toHaveTextContent("Guard error");
  });

  it("surfaces a request failure inline and renders no verdict card", async () => {
    testGuard.mockRejectedValue(new Error("boom"));
    render(<TriggerDetailPanel trigger={trigger({ guard_command: GUARD })} onSelectRun={noop} />);

    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-error")).toHaveTextContent("boom");
    expect(screen.queryByTestId("guard-test-result")).not.toBeInTheDocument();
  });

  it("shows the would-empty caveat for a prompt-required pipeline with empty input + stdout", async () => {
    testGuard.mockResolvedValue({
      outcome: "pass",
      stdout: "",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    render(
      <TriggerDetailPanel
        trigger={trigger({ guard_command: "exit 0", input_template: "" })}
        onSelectRun={noop}
        promptRequired
      />,
    );

    fireEvent.click(screen.getByTestId("guard-test-button"));

    await screen.findByTestId("guard-test-result");
    expect(screen.getByTestId("guard-test-caveat")).toBeInTheDocument();
  });

  it("hides the caveat when the pipeline is prompt-optional", async () => {
    testGuard.mockResolvedValue({
      outcome: "pass",
      stdout: "",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    render(
      <TriggerDetailPanel
        trigger={trigger({ guard_command: "exit 0", input_template: "" })}
        onSelectRun={noop}
        promptRequired={false}
      />,
    );

    fireEvent.click(screen.getByTestId("guard-test-button"));

    await screen.findByTestId("guard-test-result");
    expect(screen.queryByTestId("guard-test-caveat")).not.toBeInTheDocument();
  });
});
