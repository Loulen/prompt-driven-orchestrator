import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useLaunchTargets } from "./useLaunchTargets";
import * as api from "../api";
import type { BranchRef, PipelineListEntry } from "../types";

vi.mock("../api", () => ({
  fetchPipelines: vi.fn(),
  listBranches: vi.fn(),
}));

// #571: /repos/branches returns `[{name, kind}]`. These keep the fixtures terse.
const local = (name: string): BranchRef => ({ name, kind: "local" });
const remote = (name: string): BranchRef => ({ name, kind: "remote" });

function pipeline(over: Partial<PipelineListEntry> = {}): PipelineListEntry {
  return {
    id: "p1",
    name: "Auditor",
    scope: "repo",
    path: "/repo/.pdo/pipelines/auditor.yaml",
    node_count: 3,
    modified: null,
    variables: {},
    ...over,
  };
}

beforeEach(() => {
  vi.mocked(api.fetchPipelines).mockReset().mockResolvedValue([]);
  vi.mocked(api.listBranches)
    .mockReset()
    .mockResolvedValue([local("main"), local("dev"), local("feature-x")]);
});

function setup(open = true) {
  return renderHook(({ open }: { open: boolean }) => useLaunchTargets(open), {
    initialProps: { open },
  });
}

describe("useLaunchTargets — the pipelines", () => {
  it("fetches the list on open", async () => {
    vi.mocked(api.fetchPipelines).mockResolvedValue([pipeline()]);
    const { result } = setup();
    await waitFor(() => expect(result.current.pipelines).toHaveLength(1));
    expect(api.fetchPipelines).toHaveBeenCalledTimes(1);
  });

  it("fetches nothing while the dialog is closed", async () => {
    const { result } = setup(false);
    await act(async () => {});
    expect(api.fetchPipelines).not.toHaveBeenCalled();
    expect(result.current.pipelines).toEqual([]);
  });

  // The modal is always-mounted (#386), so a reopen has to re-read the list: a pipeline may
  // have been added, promoted or edited while it was closed.
  it("re-reads the list on every reopen", async () => {
    const { rerender } = setup();
    await waitFor(() => expect(api.fetchPipelines).toHaveBeenCalledTimes(1));
    rerender({ open: false });
    rerender({ open: true });
    await waitFor(() => expect(api.fetchPipelines).toHaveBeenCalledTimes(2));
  });

  it("groups the list by scope, repo pipelines first", async () => {
    vi.mocked(api.fetchPipelines).mockResolvedValue([
      pipeline({ id: "lib", name: "Lib", scope: "library" }),
      pipeline({ id: "repo", name: "Repo", scope: "repo" }),
      pipeline({ id: "user", name: "User", scope: "user" }),
    ]);
    const { result } = setup();
    await waitFor(() => expect(result.current.pipelines).toHaveLength(3));
    expect(result.current.repoPipelines.map((p) => p.id)).toEqual(["repo"]);
    expect(result.current.libraryPipelines.map((p) => p.id)).toEqual(["lib"]);
    expect(result.current.userPipelines.map((p) => p.id)).toEqual(["user"]);
  });

  it("resolves the selected pipeline by id, and nothing while the id is unknown", async () => {
    vi.mocked(api.fetchPipelines).mockResolvedValue([pipeline({ id: "p1" }), pipeline({ id: "p2" })]);
    const { result } = setup();
    await waitFor(() => expect(result.current.pipelines).toHaveLength(2));
    expect(result.current.selectedPipeline).toBeUndefined();

    act(() => result.current.setSelectedPipelineId("p2"));
    expect(result.current.selectedPipeline?.id).toBe("p2");

    // A pipeline that vanished from the list leaves no selection behind.
    act(() => result.current.setSelectedPipelineId("gone"));
    expect(result.current.selectedPipeline).toBeUndefined();
  });

  // A failed listing is not worth a dialog: the picker simply stays empty, which the modal
  // already renders as "No pipelines found".
  it("swallows a failed listing and leaves the picker empty", async () => {
    vi.mocked(api.fetchPipelines).mockRejectedValue(new Error("daemon unreachable"));
    const { result } = setup();
    await act(async () => {});
    expect(result.current.pipelines).toEqual([]);
  });

  // What the promote button calls: the star moves from "repo" to "library" server-side, so
  // the list has to be re-read for it to show.
  it("re-reads the list on demand", async () => {
    const { result } = setup();
    await waitFor(() => expect(api.fetchPipelines).toHaveBeenCalledTimes(1));
    vi.mocked(api.fetchPipelines).mockResolvedValue([pipeline({ scope: "library" })]);
    await act(async () => {
      result.current.loadPipelines();
    });
    await waitFor(() => expect(result.current.libraryPipelines).toHaveLength(1));
  });
});

describe("useLaunchTargets — the branches", () => {
  async function load(
    result: { current: ReturnType<typeof useLaunchTargets> },
    repoPath: string,
  ) {
    await act(async () => {
      await result.current.loadBranches(repoPath);
    });
  }

  it("lists the target repo's branches", async () => {
    const { result } = setup();
    await load(result, "/home/user/project");
    expect(api.listBranches).toHaveBeenCalledWith("/home/user/project");
    expect(result.current.branches).toEqual([
      local("main"),
      local("dev"),
      local("feature-x"),
    ]);
    expect(result.current.branchesLoading).toBe(false);
  });

  it("flags the load while it is in flight", async () => {
    let release!: (branches: BranchRef[]) => void;
    vi.mocked(api.listBranches).mockReturnValue(
      new Promise<BranchRef[]>((resolve) => {
        release = resolve;
      }),
    );
    const { result } = setup();
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.loadBranches("/home/user/project");
    });
    expect(result.current.branchesLoading).toBe(true);

    await act(async () => {
      release([local("main")]);
      await pending;
    });
    expect(result.current.branchesLoading).toBe(false);
    expect(result.current.branches).toEqual([local("main")]);
  });

  /**
   * #454: the selection is re-made whenever the held branch is not one THIS repo has. The
   * old `!sourceBranch` guard only ever seeded an empty field, so switching repos kept a
   * branch the new one lacks — and a `<select>` whose value matches no option renders its
   * FIRST option, so the field DISPLAYED `master` while the state still held `main`, and the
   * launch failed with `branch 'main' does not exist`.
   */
  it("re-selects when the held branch is not one this repo has (#454)", async () => {
    const { result } = setup();
    await load(result, "/home/user/project-a");
    expect(result.current.sourceBranch).toBe("main");

    vi.mocked(api.listBranches).mockResolvedValue([local("master")]);
    await load(result, "/home/user/project-b");
    expect(result.current.sourceBranch).toBe("master");
  });

  // The other half of #454: testing membership subsumes the empty case WITHOUT throwing
  // away a deliberate choice the new repo honours.
  it("keeps a deliberate choice the new repo still offers (#454)", async () => {
    const { result } = setup();
    await load(result, "/home/user/project-a");
    act(() => result.current.setSourceBranch("feature-x"));

    vi.mocked(api.listBranches).mockResolvedValue([local("main"), local("feature-x")]);
    await load(result, "/home/user/project-b");
    expect(result.current.sourceBranch).toBe("feature-x");
  });

  it("prefers main, then master, then whatever comes first", async () => {
    const { result } = setup();

    vi.mocked(api.listBranches).mockResolvedValue([
      local("dev"),
      local("master"),
      local("main"),
    ]);
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("main");

    vi.mocked(api.listBranches).mockResolvedValue([local("dev"), local("master")]);
    await load(result, "/b");
    expect(result.current.sourceBranch).toBe("master");

    vi.mocked(api.listBranches).mockResolvedValue([local("topic"), local("other")]);
    await load(result, "/c");
    expect(result.current.sourceBranch).toBe("topic");
  });

  // #571: the default is locality-aware. A remote is NEVER chosen while a local
  // exists — even when a remote `origin/main` is present and the only local is
  // `master`. This is the exact bug the #454 rule exists to prevent, now that
  // remotes share the list.
  it("never defaults to a remote while a local exists (#571)", async () => {
    const { result } = setup();
    vi.mocked(api.listBranches).mockResolvedValue([
      local("master"),
      remote("origin/main"),
      remote("origin/master"),
    ]);
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("master");
  });

  // #571: with zero local branches (a remote-only repo state), fall through to a
  // remote — preferring `/main`, then `/master`, then the first remote.
  it("falls back to a remote only when there is no local (#571)", async () => {
    const { result } = setup();

    vi.mocked(api.listBranches).mockResolvedValue([
      remote("origin/dev"),
      remote("origin/master"),
      remote("origin/main"),
    ]);
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("origin/main");

    vi.mocked(api.listBranches).mockResolvedValue([
      remote("origin/dev"),
      remote("origin/master"),
    ]);
    await load(result, "/b");
    expect(result.current.sourceBranch).toBe("origin/master");

    vi.mocked(api.listBranches).mockResolvedValue([
      remote("upstream/topic"),
      remote("origin/other"),
    ]);
    await load(result, "/c");
    expect(result.current.sourceBranch).toBe("upstream/topic");
  });

  // #571: membership is on `name`, so a remote branch the user picked survives a
  // re-list that still offers it (no needless re-seed to the default local).
  it("keeps a chosen remote branch when the repo still offers it (#571)", async () => {
    const { result } = setup();
    vi.mocked(api.listBranches).mockResolvedValue([
      local("main"),
      remote("origin/feature-x"),
    ]);
    await load(result, "/a");
    act(() => result.current.setSourceBranch("origin/feature-x"));

    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("origin/feature-x");
  });

  it("re-selects nothing for a repo that lists no branch at all", async () => {
    const { result } = setup();
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("main");

    vi.mocked(api.listBranches).mockResolvedValue([]);
    await load(result, "/b");
    expect(result.current.branches).toEqual([]);
    expect(result.current.sourceBranch).toBe("main");
  });

  it("empties the list when the listing fails", async () => {
    const { result } = setup();
    await load(result, "/a");
    vi.mocked(api.listBranches).mockRejectedValue(new Error("not a git repository"));
    await load(result, "/b");
    expect(result.current.branches).toEqual([]);
    expect(result.current.branchesLoading).toBe(false);
  });

  it("clearBranches drops the list and the selection together", async () => {
    const { result } = setup();
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("main");

    act(() => result.current.clearBranches());
    expect(result.current.branches).toEqual([]);
    expect(result.current.sourceBranch).toBe("");
  });
});
