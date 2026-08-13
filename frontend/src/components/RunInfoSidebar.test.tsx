import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import RunInfoSidebar from "./RunInfoSidebar";
import type { RunState, RunStatus, RepoPin } from "../types";
import { editRunRepos } from "../api";

vi.mock("../api", () => ({
  editRunRepos: vi.fn(async () => ({ kind: "ok", run: {} as RunState })),
}));

// The store is a zustand hook (selector in, slice out) — stub it to a stable empty
// list so `SecondaryRepoRow`'s combobox has something to read.
vi.mock("../stores/recentReposStore", () => ({
  useRecentReposStore: (sel: (s: { recentRepos: string[] }) => unknown) =>
    sel({ recentRepos: [] }),
}));

const editRunReposMock = vi.mocked(editRunRepos);

beforeEach(() => {
  editRunReposMock.mockClear();
});

function makeRun(status: RunStatus, failure_reason?: string): RunState {
  return {
    run_id: "20260704-000000-abc1234",
    pipeline_name: "My Pipeline",
    status,
    failure_reason,
    nodes: {},
    node_defs: [],
    edges: [],
  } as unknown as RunState;
}

function makeMultiRepoRun(
  status: RunStatus,
  target_repos: RepoPin[],
): RunState {
  return {
    ...makeRun(status),
    target_repo: "/repos/primary",
    target_repos,
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

  // #503: this is the panel a user reaches by clicking a red dot. It used to talk
  // about pipeline editing and say nothing at all about the failure.
  it("states why a failed run failed", () => {
    render(
      <RunInfoSidebar
        run={makeRun("failed", "merge conflict on ship: 20 conflicting file(s)")}
      />,
    );
    const box = screen.getByTestId("run-failure-reason");
    expect(box.textContent).toContain("Failed");
    expect(box.textContent).toContain("20 conflicting file(s)");
  });

  it("names the terminal it is explaining", () => {
    render(<RunInfoSidebar run={makeRun("halted", "stop condition met")} />);
    expect(screen.getByTestId("run-failure-reason").textContent).toContain("Halted");
  });

  it("shows no failure box on a green or live run", () => {
    render(<RunInfoSidebar run={makeRun("completed")} />);
    expect(screen.queryByTestId("run-failure-reason")).toBeNull();
  });

  // #465 slice 2 — the Repositories section.
  it("shows no Repositories section for a mono-repo run (no target_repo)", () => {
    render(<RunInfoSidebar run={makeRun("running")} />);
    expect(screen.queryByTestId("run-repositories")).toBeNull();
  });

  it("renders the primary locked and the secondaries with a remove button", () => {
    const run = makeMultiRepoRun("running", [
      { repo: "/repos/lib", alias: "lib", sha: "cafebabe1234", base_branch: "main" },
    ]);
    render(<RunInfoSidebar run={run} onEdited={() => {}} />);

    // Primary is locked: badge present, no remove button on its row.
    const primary = screen.getByTestId("primary-repo-row");
    expect(primary.textContent).toContain("/repos/primary");
    expect(screen.getByTestId("primary-repo-badge").textContent).toBe("PRIMARY");

    // Secondary shows its path, short sha and a remove button.
    const secondary = screen.getByTestId("secondary-repo-lib");
    expect(secondary.textContent).toContain("/repos/lib");
    expect(secondary.textContent).toContain("cafebabe");
    expect(screen.getByTestId("remove-secondary-repo-lib")).toBeInTheDocument();
  });

  it("clicking a secondary's X calls editRunRepos({ remove: [alias] })", async () => {
    const onEdited = vi.fn();
    const run = makeMultiRepoRun("running", [
      { repo: "/repos/lib", alias: "lib", sha: "cafebabe1234" },
    ]);
    render(<RunInfoSidebar run={run} onEdited={onEdited} />);

    fireEvent.click(screen.getByTestId("remove-secondary-repo-lib"));

    expect(editRunReposMock).toHaveBeenCalledWith(run.run_id, { remove: ["lib"] });
    await waitFor(() => expect(onEdited).toHaveBeenCalled());
  });

  it("the + Add repository button reveals a self-validating draft row", () => {
    const run = makeMultiRepoRun("running", []);
    render(<RunInfoSidebar run={run} onEdited={() => {}} />);

    expect(screen.queryByTestId("secondary-repo-draft")).toBeNull();
    fireEvent.click(screen.getByTestId("add-secondary-repo"));
    expect(screen.getByTestId("secondary-repo-draft")).toBeInTheDocument();
  });

  it("carries the spawn-time visibility note on a live run", () => {
    const run = makeMultiRepoRun("running", []);
    render(<RunInfoSidebar run={run} onEdited={() => {}} />);
    expect(screen.getByTestId("spawn-visibility-note").textContent).toContain(
      "launched after",
    );
  });

  it("a terminal run's list is frozen: no remove, no add (#221)", () => {
    const run = makeMultiRepoRun("completed", [
      { repo: "/repos/lib", alias: "lib", sha: "cafebabe1234" },
    ]);
    render(<RunInfoSidebar run={run} />);

    // The section and the secondary still render (read-only)...
    expect(screen.getByTestId("run-repositories")).toBeInTheDocument();
    expect(screen.getByTestId("secondary-repo-lib")).toBeInTheDocument();
    // ...but there is no way to mutate it.
    expect(screen.queryByTestId("remove-secondary-repo-lib")).toBeNull();
    expect(screen.queryByTestId("add-secondary-repo")).toBeNull();
    expect(screen.queryByTestId("spawn-visibility-note")).toBeNull();
  });
});
