import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import type { RunState } from "../types";

// The badge/banner live in the InfoTab header, above DiffSection. Mock the heavy
// children (network-fetching diff, tmux terminal) so the test stays focused on the
// #410 sandbox surface and never touches the network.
vi.mock("./DiffSection", () => ({ default: () => null }));
vi.mock("./TmuxTerminal", () => ({ default: () => null }));

import PipelineInfoPanel from "./PipelineInfoPanel";

function makeRun(overrides: Partial<RunState> = {}): RunState {
  return {
    run_id: "run-abc1234567",
    status: "running",
    pipeline_name: "Test Pipeline",
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

function renderPanel(run: RunState | null) {
  return render(
    <PipelineInfoPanel
      run={run}
      pipeline={null}
      libraryPipelines={[]}
      onLibraryChanged={() => {}}
      onClose={() => {}}
    />,
  );
}

describe("PipelineInfoPanel — sandbox surface (#410)", () => {
  it("shows the sandbox badge for a sandboxed run (pure)", () => {
    renderPanel(makeRun({ sandbox: "pure" }));
    const badge = screen.getByTestId("sandbox-badge");
    expect(badge).toHaveTextContent(/sandbox:\s*pure/i);
  });

  it("shows the sandbox badge for a copy run", () => {
    renderPanel(makeRun({ sandbox: "copy" }));
    expect(screen.getByTestId("sandbox-badge")).toHaveTextContent(/sandbox:\s*copy/i);
  });

  it("omits the badge for an off/host run", () => {
    renderPanel(makeRun({ sandbox: "off" }));
    expect(screen.queryByTestId("sandbox-badge")).not.toBeInTheDocument();
  });

  it("omits the badge when sandbox is absent (historical/host run)", () => {
    renderPanel(makeRun());
    expect(screen.queryByTestId("sandbox-badge")).not.toBeInTheDocument();
  });

  it("shows the preparation banner while sandbox_prep is pending", () => {
    renderPanel(makeRun({ sandbox: "pure", sandbox_prep: "pending" }));
    expect(screen.getByTestId("sandbox-prep-banner")).toHaveTextContent(/preparing the sandbox/i);
  });

  it("hides the preparation banner once sandbox_prep is ready", () => {
    renderPanel(makeRun({ sandbox: "pure", sandbox_prep: "ready" }));
    expect(screen.queryByTestId("sandbox-prep-banner")).not.toBeInTheDocument();
    // The badge stays visible after prep completes.
    expect(screen.getByTestId("sandbox-badge")).toBeInTheDocument();
  });
});
