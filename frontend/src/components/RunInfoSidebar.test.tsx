import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import RunInfoSidebar from "./RunInfoSidebar";
import type { RunState, RunStatus } from "../types";

function makeRun(status: RunStatus): RunState {
  return {
    run_id: "20260704-000000-abc1234",
    pipeline_name: "My Pipeline",
    status,
    nodes: {},
    node_defs: [],
    edges: [],
  } as unknown as RunState;
}

describe("RunInfoSidebar", () => {
  it("shows the sync-to-template note for a live run", () => {
    render(<RunInfoSidebar run={makeRun("running")} />);
    const note = screen.getByTestId("run-info-note");
    expect(note.textContent).toContain("changes sync to template");
    expect(note.textContent).not.toContain("read-only");
  });

  it("shows a read-only archived note for an archived run (#315)", () => {
    render(<RunInfoSidebar run={makeRun("archived")} />);
    const note = screen.getByTestId("run-info-note");
    expect(note.textContent).toContain("Archived run");
    expect(note.textContent).toContain("read-only");
    expect(note.textContent).not.toContain("changes sync to template");
  });

  it("renders the pipeline name and run id in both states", () => {
    render(<RunInfoSidebar run={makeRun("archived")} />);
    expect(screen.getByText("My Pipeline")).toBeInTheDocument();
    expect(screen.getByText("20260704-000000-abc1234")).toBeInTheDocument();
  });
});
