import { render, screen, fireEvent, waitFor, within } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import RepositoriesSection from "./RepositoriesSection";
import type { RunState, RunStatus, RepoPin } from "../types";
import { editRunRepos, validateRepo, listBranches } from "../api";

vi.mock("../api", () => ({
  editRunRepos: vi.fn(async () => ({ kind: "ok", run: {} as RunState })),
  // A draft `SecondaryRepoRow` self-validates via these; stub them so a draft can
  // reach `valid === true` (which is what reveals the read-only checkbox).
  validateRepo: vi.fn(async () => ({ valid: true })),
  // #802: both branch verbs answer `{branches, last_fetch_at, fetch_error}` — the
  // row fetches its own repo's remotes beside the list.
  listBranches: vi.fn(async () => ({
    branches: [{ name: "main", kind: "local" as const }],
    last_fetch_at: null,
    fetch_error: null,
  })),
  fetchRemotes: vi.fn(async () => ({
    branches: [{ name: "main", kind: "local" as const }],
    last_fetch_at: null,
    fetch_error: null,
  })),
}));

// The store is a zustand hook (selector in, slice out) — stub it to a stable empty
// list so `SecondaryRepoRow`'s combobox has something to read.
vi.mock("../stores/recentReposStore", () => ({
  useRecentReposStore: (sel: (s: { recentRepos: string[] }) => unknown) =>
    sel({ recentRepos: [] }),
}));

const editRunReposMock = vi.mocked(editRunRepos);
const validateRepoMock = vi.mocked(validateRepo);
const listBranchesMock = vi.mocked(listBranches);

beforeEach(() => {
  editRunReposMock.mockClear();
  validateRepoMock.mockClear();
  listBranchesMock.mockClear();
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

// #752: the Repositories section moved from the standalone `RunInfoSidebar`
// (deleted) into the Run panel's Repositories tab. Same content, same test ids.
describe("RepositoriesSection (Run panel · Repositories tab)", () => {
  // #465 slice 2 — the Repositories section.
  it("renders the primary locked and the secondaries with a remove button", () => {
    const run = makeMultiRepoRun("running", [
      { repo: "/repos/lib", alias: "lib", sha: "cafebabe1234", base_branch: "main" },
    ]);
    render(<RepositoriesSection run={run} onEdited={() => {}} />);

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

  // ADR-0047: the badge reflects the per-repo mode. A writable pin (the default,
  // no `read_only`) reads WRITABLE; an opted-in read-only pin reads READ-ONLY.
  it("badges each secondary by its writable/read-only mode", () => {
    const run = makeMultiRepoRun("running", [
      { repo: "/repos/rw", alias: "rw", sha: "aaaa1111" },
      { repo: "/repos/ro", alias: "ro", sha: "bbbb2222", read_only: true },
    ]);
    render(<RepositoriesSection run={run} onEdited={() => {}} />);
    expect(screen.getByTestId("secondary-repo-mode-rw").textContent).toBe("WRITABLE");
    expect(screen.getByTestId("secondary-repo-mode-ro").textContent).toBe("READ-ONLY");
  });

  it("adds a read-only draft with read_only:true in the PATCH (ADR-0047)", async () => {
    const run = makeMultiRepoRun("running", []);
    render(<RepositoriesSection run={run} onEdited={() => {}} />);

    fireEvent.click(screen.getByTestId("add-secondary-repo"));
    const draft = screen.getByTestId("secondary-repo-draft");
    fireEvent.change(within(draft).getByTestId("target-repo-input"), {
      target: { value: "/repos/newlib" },
    });

    // The row debounces then self-validates; the checkbox appears once valid.
    const checkbox = await within(draft).findByTestId(
      "secondary-readonly-0",
      {},
      { timeout: 2000 },
    );
    fireEvent.click(checkbox);

    const confirm = screen.getByTestId("confirm-add-secondary-repo");
    await waitFor(() => expect(confirm).toBeEnabled());
    fireEvent.click(confirm);

    await waitFor(() => expect(editRunReposMock).toHaveBeenCalled());
    expect(editRunReposMock).toHaveBeenCalledWith(run.run_id, {
      add: [expect.objectContaining({ repo: "/repos/newlib", read_only: true })],
    });
  });

  it("clicking a secondary's X calls editRunRepos({ remove: [alias] })", async () => {
    const onEdited = vi.fn();
    const run = makeMultiRepoRun("running", [
      { repo: "/repos/lib", alias: "lib", sha: "cafebabe1234" },
    ]);
    render(<RepositoriesSection run={run} onEdited={onEdited} />);

    fireEvent.click(screen.getByTestId("remove-secondary-repo-lib"));

    expect(editRunReposMock).toHaveBeenCalledWith(run.run_id, { remove: ["lib"] });
    await waitFor(() => expect(onEdited).toHaveBeenCalled());
  });

  it("the + Add repository button reveals a self-validating draft row", () => {
    const run = makeMultiRepoRun("running", []);
    render(<RepositoriesSection run={run} onEdited={() => {}} />);

    expect(screen.queryByTestId("secondary-repo-draft")).toBeNull();
    fireEvent.click(screen.getByTestId("add-secondary-repo"));
    expect(screen.getByTestId("secondary-repo-draft")).toBeInTheDocument();
  });

  it("carries the spawn-time visibility note on a live run", () => {
    const run = makeMultiRepoRun("running", []);
    render(<RepositoriesSection run={run} onEdited={() => {}} />);
    expect(screen.getByTestId("spawn-visibility-note").textContent).toContain(
      "launched after",
    );
  });

  it("a terminal run's list is frozen: no remove, no add (#221)", () => {
    const run = makeMultiRepoRun("completed", [
      { repo: "/repos/lib", alias: "lib", sha: "cafebabe1234" },
    ]);
    render(<RepositoriesSection run={run} />);

    // The section and the secondary still render (read-only)...
    expect(screen.getByTestId("run-repositories")).toBeInTheDocument();
    expect(screen.getByTestId("secondary-repo-lib")).toBeInTheDocument();
    // ...but there is no way to mutate it.
    expect(screen.queryByTestId("remove-secondary-repo-lib")).toBeNull();
    expect(screen.queryByTestId("add-secondary-repo")).toBeNull();
    expect(screen.queryByTestId("spawn-visibility-note")).toBeNull();
  });
});

/**
 * #804: the primary pin now states its cut point like the secondaries always did,
 * and marks it when a Trigger's pre-cut fetch failed. A complement to the Info
 * tab's sentence, not a replacement — nobody opens Repositories to find out why a
 * Run looks odd, but once here the pin should not be the only one saying nothing.
 */
describe("the primary repo's cut point (#804)", () => {
  const FETCH_ERROR = { kind: "network", message: "fatal: unable to access …: Could not resolve host" };

  function makeCutRun(over: Partial<RunState> = {}): RunState {
    return {
      ...makeMultiRepoRun("running", []),
      source_branch: "origin/main",
      fork_sha: "0c051abf9911223344",
      ...over,
    } as RunState;
  }

  it("names the branch and the commit the Run was cut from", () => {
    render(<RepositoriesSection run={makeCutRun()} />);
    const cut = screen.getByTestId("primary-repo-cut");
    expect(cut).toHaveTextContent("origin/main");
    // The short sha, as the secondaries show theirs.
    expect(cut).toHaveTextContent("0c051abf");
    expect(cut).not.toHaveTextContent("0c051abf99");
  });

  it("falls back to HEAD when the Run named no source branch", () => {
    render(<RepositoriesSection run={makeCutRun({ source_branch: null })} />);
    expect(screen.getByTestId("primary-repo-cut")).toHaveTextContent("HEAD");
  });

  it("marks the pin when the pre-cut fetch failed, with git's reason on hover", () => {
    render(<RepositoriesSection run={makeCutRun({ source_fetch_error: FETCH_ERROR })} />);
    const mark = screen.getByTestId("primary-repo-fetch-failed");
    expect(mark).toHaveAccessibleName(
      `Fetch failed before the cut — cut from local state. ${FETCH_ERROR.message}`,
    );
    expect(mark).toHaveAttribute("title", expect.stringContaining("cut from local state"));
  });

  it("leaves the pin unmarked on every other Run", () => {
    render(<RepositoriesSection run={makeCutRun()} />);
    expect(screen.queryByTestId("primary-repo-fetch-failed")).toBeNull();
  });
});
