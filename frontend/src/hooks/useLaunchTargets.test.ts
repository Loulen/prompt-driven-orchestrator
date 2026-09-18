import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act, waitFor } from "@testing-library/react";
import { useLaunchTargets } from "./useLaunchTargets";
import * as api from "../api";
import type { BranchList, BranchRef, FastForwardOutcome, PipelineListEntry } from "../types";

vi.mock("../api", () => ({
  fetchPipelines: vi.fn(),
  listBranches: vi.fn(),
  fetchRemotes: vi.fn(),
  fastForwardBranch: vi.fn(),
}));

// #571: a branch is `{name, kind}`. These keep the fixtures terse.
const local = (name: string): BranchRef => ({ name, kind: "local" });
const remote = (name: string): BranchRef => ({ name, kind: "remote" });

/** #802: both branch verbs answer the list PLUS the date it is good as of. */
function branchList(branches: BranchRef[], over: Partial<BranchList> = {}): BranchList {
  return { branches, last_fetch_at: null, fetch_error: null, ...over };
}

/**
 * Point BOTH branch verbs at the same repo state. The daemon's fetch answers the
 * SAME list, refreshed — a fixture where only `listBranches` knows the branches
 * would have the fetch wipe them a beat later, which no real daemon does. Tests
 * about the fetch itself re-point `fetchRemotes` afterwards.
 */
function listBranchesReturns(branches: BranchRef[], over: Partial<BranchList> = {}) {
  vi.mocked(api.fetchRemotes).mockResolvedValue(branchList(branches, over));
  return vi.mocked(api.listBranches).mockResolvedValue(branchList(branches, over));
}

function pipeline(over: Partial<PipelineListEntry> = {}): PipelineListEntry {
  return {
    id: "p1",
    name: "Auditor",
    scope: "instance",
    path: "/repo/.pdo/pipelines/auditor.yaml",
    node_count: 3,
    modified: null,
    variables: {},
    ...over,
  };
}

beforeEach(() => {
  vi.mocked(api.fetchPipelines).mockReset().mockResolvedValue([]);
  vi.mocked(api.listBranches).mockReset();
  vi.mocked(api.fetchRemotes).mockReset();
  vi.mocked(api.fastForwardBranch).mockReset();
  listBranchesReturns([local("main"), local("dev"), local("feature-x")]);
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

  it("keeps the daemon's instance pipeline list flat", async () => {
    vi.mocked(api.fetchPipelines).mockResolvedValue([
      pipeline({ id: "first", name: "First" }),
      pipeline({ id: "second", name: "Second" }),
    ]);
    const { result } = setup();
    await waitFor(() => expect(result.current.pipelines).toHaveLength(2));
    expect(result.current.pipelines.map((p) => p.id)).toEqual(["first", "second"]);
    expect(result.current).not.toHaveProperty("repoPipelines");
    expect(result.current).not.toHaveProperty("libraryPipelines");
    expect(result.current).not.toHaveProperty("userPipelines");
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

  it("re-reads the list on demand", async () => {
    const { result } = setup();
    await waitFor(() => expect(api.fetchPipelines).toHaveBeenCalledTimes(1));
    vi.mocked(api.fetchPipelines).mockResolvedValue([pipeline()]);
    await act(async () => {
      result.current.loadPipelines();
    });
    await waitFor(() => expect(result.current.pipelines).toHaveLength(1));
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
    let release!: (list: BranchList) => void;
    // Both verbs answer the same repo state (see `listBranchesReturns`); only the
    // list is held open, so the flag is about the LIST arriving.
    vi.mocked(api.fetchRemotes).mockResolvedValue(branchList([local("main")]));
    vi.mocked(api.listBranches).mockReturnValue(
      new Promise<BranchList>((resolve) => {
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
      release(branchList([local("main")]));
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

    listBranchesReturns([local("master")]);
    await load(result, "/home/user/project-b");
    expect(result.current.sourceBranch).toBe("master");
  });

  // The other half of #454: testing membership subsumes the empty case WITHOUT throwing
  // away a deliberate choice the new repo honours.
  it("keeps a deliberate choice the new repo still offers (#454)", async () => {
    const { result } = setup();
    await load(result, "/home/user/project-a");
    act(() => result.current.setSourceBranch("feature-x"));

    listBranchesReturns([local("main"), local("feature-x")]);
    await load(result, "/home/user/project-b");
    expect(result.current.sourceBranch).toBe("feature-x");
  });

  it("prefers main, then master, then whatever comes first", async () => {
    const { result } = setup();

    listBranchesReturns([
      local("dev"),
      local("master"),
      local("main"),
    ]);
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("main");

    listBranchesReturns([local("dev"), local("master")]);
    await load(result, "/b");
    expect(result.current.sourceBranch).toBe("master");

    listBranchesReturns([local("topic"), local("other")]);
    await load(result, "/c");
    expect(result.current.sourceBranch).toBe("topic");
  });

  // #571: the default is locality-aware. A remote is NEVER chosen while a local
  // exists — even when a remote `origin/main` is present and the only local is
  // `master`. This is the exact bug the #454 rule exists to prevent, now that
  // remotes share the list.
  it("never defaults to a remote while a local exists (#571)", async () => {
    const { result } = setup();
    listBranchesReturns([
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

    listBranchesReturns([
      remote("origin/dev"),
      remote("origin/master"),
      remote("origin/main"),
    ]);
    await load(result, "/a");
    expect(result.current.sourceBranch).toBe("origin/main");

    listBranchesReturns([
      remote("origin/dev"),
      remote("origin/master"),
    ]);
    await load(result, "/b");
    expect(result.current.sourceBranch).toBe("origin/master");

    listBranchesReturns([
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
    listBranchesReturns([
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

    listBranchesReturns([]);
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

/**
 * #802/ADR-0070. The écart amont is only worth the date it carries, so loading a
 * repo's branches also fetches its remotes. Everything below is about that fetch
 * NEVER being allowed to cost anything: not the list, not the selection, not the
 * ability to launch.
 */
describe("useLaunchTargets — the fetch on open and on repo change (#802)", () => {
  async function load(
    result: { current: ReturnType<typeof useLaunchTargets> },
    repoPath: string,
  ) {
    await act(async () => {
      await result.current.loadBranches(repoPath);
    });
  }

  it("fetches the repo's remotes when its branches are loaded", async () => {
    const { result } = setup();
    await load(result, "/home/user/project");
    expect(api.fetchRemotes).toHaveBeenCalledWith("/home/user/project");
  });

  it("re-fetches when the target repo changes", async () => {
    const { result } = setup();
    await load(result, "/a");
    await load(result, "/b");
    expect(vi.mocked(api.fetchRemotes).mock.calls.map(([p]) => p)).toEqual(["/a", "/b"]);
  });

  it("lands the refreshed list and its date in place", async () => {
    const { result } = setup();
    vi.mocked(api.fetchRemotes).mockResolvedValue(
      branchList([{ ...local("main"), upstream: "origin/main", ahead: 0, behind: 3 }], {
        last_fetch_at: "2026-09-18T09:00:00Z",
      }),
    );
    await load(result, "/a");
    await waitFor(() => expect(result.current.branches[0]?.behind).toBe(3));
    expect(result.current.lastFetchAt).toBe("2026-09-18T09:00:00Z");
    expect(result.current.fetchError).toBeNull();
    expect(result.current.fetching).toBe(false);
  });

  /**
   * The load and the fetch race by construction (the fetch is started first and
   * awaited last). Whichever lands second must not undo the selection the first
   * one seeded — a refetch keeps the user's pick, always.
   */
  it("keeps the selection across a refetch", async () => {
    const { result } = setup();
    await load(result, "/a");
    act(() => result.current.setSourceBranch("feature-x"));

    vi.mocked(api.fetchRemotes).mockResolvedValue(
      branchList([local("main"), local("dev"), local("feature-x")], {
        last_fetch_at: "2026-09-18T09:00:00Z",
      }),
    );
    await act(async () => {
      result.current.refetchRemotes();
    });
    await waitFor(() => expect(result.current.lastFetchAt).toBe("2026-09-18T09:00:00Z"));
    expect(result.current.sourceBranch).toBe("feature-x");
  });

  /**
   * ADR-0070 §1. A repo with no remote, an offline machine and a missing key are
   * all legitimate; the only consequence is that the écart becomes unknown.
   */
  it("keeps the list, the selection and the date when the fetch fails", async () => {
    const { result } = setup();
    vi.mocked(api.fetchRemotes).mockResolvedValue(
      branchList([local("main"), local("dev"), local("feature-x")], {
        last_fetch_at: "2026-09-16T08:00:00Z",
        fetch_error: { kind: "network", message: "ssh: Could not resolve hostname" },
      }),
    );
    await load(result, "/a");
    await waitFor(() => expect(result.current.fetchError?.kind).toBe("network"));
    expect(result.current.branches).toHaveLength(3);
    expect(result.current.sourceBranch).toBe("main");
    expect(result.current.lastFetchAt).toBe("2026-09-16T08:00:00Z");
  });

  /**
   * The list and the fetch are started together and answer in either order. The
   * fetch returns the SAME list refreshed, so a late `listBranches` landing on top
   * would put the pre-fetch écart back on screen — stale numbers presented as
   * current, which is the whole bug this feature removes.
   */
  it("does not let a late plain list overwrite a landed fetch", async () => {
    let releaseList!: (list: BranchList) => void;
    vi.mocked(api.listBranches).mockReturnValue(
      new Promise<BranchList>((resolve) => {
        releaseList = resolve;
      }),
    );
    vi.mocked(api.fetchRemotes).mockResolvedValue(
      branchList([{ ...local("main"), upstream: "origin/main", ahead: 0, behind: 3 }], {
        last_fetch_at: "2026-09-18T09:00:00Z",
      }),
    );

    const { result } = setup();
    let pending!: Promise<void>;
    act(() => {
      pending = result.current.loadBranches("/a");
    });
    await waitFor(() => expect(result.current.branches[0]?.behind).toBe(3));

    await act(async () => {
      // The pre-fetch answer, arriving last: `main` with no upstream knowledge.
      releaseList(branchList([local("main")]));
      await pending;
    });
    expect(result.current.branches[0]?.behind).toBe(3);
    expect(result.current.lastFetchAt).toBe("2026-09-18T09:00:00Z");
  });

  it("survives a fetch request that throws outright", async () => {
    const { result } = setup();
    vi.mocked(api.fetchRemotes).mockRejectedValue(new Error("daemon unreachable"));
    await load(result, "/a");
    await waitFor(() => expect(result.current.fetchError?.message).toContain("daemon unreachable"));
    expect(result.current.branches).toHaveLength(3);
    expect(result.current.fetching).toBe(false);
  });

  /**
   * The user can change repo while a fetch is in the air. A late answer about the
   * PREVIOUS repo landing on the current one is the multi-repo shape of the #454
   * stale-branch bug: the field would show branches of a repo nobody selected.
   */
  it("ignores a fetch that answers about a repo the user has left", async () => {
    let releaseSlow!: (list: BranchList) => void;
    vi.mocked(api.fetchRemotes).mockReturnValueOnce(
      new Promise<BranchList>((resolve) => {
        releaseSlow = resolve;
      }),
    );
    const { result } = setup();
    await load(result, "/slow");

    listBranchesReturns([local("other")]);
    vi.mocked(api.fetchRemotes).mockResolvedValue(branchList([local("other")]));
    await load(result, "/fast");
    expect(result.current.branches).toEqual([local("other")]);

    await act(async () => {
      releaseSlow(
        branchList([local("stale-a"), local("stale-b")], {
          last_fetch_at: "1999-01-01T00:00:00Z",
        }),
      );
    });
    expect(result.current.branches).toEqual([local("other")]);
    expect(result.current.lastFetchAt).not.toBe("1999-01-01T00:00:00Z");
  });

  it("clearBranches drops the freshness state with the list", async () => {
    const { result } = setup();
    vi.mocked(api.fetchRemotes).mockResolvedValue(
      branchList([local("main")], {
        last_fetch_at: "2026-09-18T09:00:00Z",
        fetch_error: { kind: "auth", message: "Permission denied" },
      }),
    );
    await load(result, "/a");
    await waitFor(() => expect(result.current.fetchError).not.toBeNull());

    act(() => result.current.clearBranches());
    expect(result.current.lastFetchAt).toBeNull();
    expect(result.current.fetchError).toBeNull();
  });

  it("refetches nothing when no repo is loaded", async () => {
    const { result } = setup();
    await act(async () => {
      result.current.refetchRemotes();
    });
    expect(api.fetchRemotes).not.toHaveBeenCalled();
  });
});

/**
 * #803/ADR-0070 §2 — the fast-forward's race rules, which the popover must not
 * have to remember. Nothing here asserts on a request shape: what matters is what
 * the numbers on screen say afterwards.
 */
describe("useLaunchTargets — the fast-forward (#803)", () => {
  async function load(
    result: { current: ReturnType<typeof useLaunchTargets> },
    repoPath: string,
  ) {
    await act(async () => {
      await result.current.loadBranches(repoPath);
    });
  }

  const ffDone = (branches: BranchRef[]) => ({
    kind: "done" as const,
    result: {
      branch: "main",
      upstream: "origin/main",
      from: "a1b2c3d",
      to: "e4f5a6b",
      commits: 3,
    },
    list: branchList(branches),
  });

  it("refreshes the list from the 200, so the écart it erased disappears", async () => {
    const { result } = setup();
    listBranchesReturns([{ name: "main", kind: "local", upstream: "origin/main", ahead: 0, behind: 3 }]);
    await load(result, "/a");
    await waitFor(() => expect(result.current.branches).toHaveLength(1));

    vi.mocked(api.fastForwardBranch).mockResolvedValue(
      ffDone([{ name: "main", kind: "local", upstream: "origin/main", ahead: 0, behind: 0 }]),
    );
    await act(async () => {
      await result.current.fastForward("main");
    });
    expect(result.current.branches[0].behind).toBe(0);
  });

  /**
   * The two benign races. The 409 carries the SAME refreshed list, and applying it
   * is what lets `up_to_date` heal into "level" instead of an error card.
   */
  it("refreshes the list from a 409 too", async () => {
    const { result } = setup();
    listBranchesReturns([{ name: "main", kind: "local", upstream: "origin/main", ahead: 0, behind: 3 }]);
    await load(result, "/a");
    await waitFor(() => expect(result.current.branches).toHaveLength(1));

    vi.mocked(api.fastForwardBranch).mockResolvedValue({
      kind: "refused",
      refusal: { reason: "up_to_date", message: "already up to date" },
      list: branchList([{ name: "main", kind: "local", upstream: "origin/main", ahead: 0, behind: 0 }]),
    });
    await act(async () => {
      await result.current.fastForward("main");
    });
    expect(result.current.branches[0].behind).toBe(0);
  });

  /**
   * A fast-forward makes no remote attempt, so its unfailing `fetch_error: null`
   * says nothing about whether the last fetch worked — erasing a real failure here
   * would turn "gap unknown" back into numbers presented as current.
   */
  it("never clears a live fetch failure", async () => {
    const { result } = setup();
    listBranchesReturns([local("main")], {
      fetch_error: { kind: "network", message: "could not resolve host" },
    });
    await load(result, "/a");
    await waitFor(() => expect(result.current.fetchError).not.toBeNull());

    vi.mocked(api.fastForwardBranch).mockResolvedValue(ffDone([local("main")]));
    await act(async () => {
      await result.current.fastForward("main");
    });
    expect(result.current.fetchError).not.toBeNull();
  });

  it("drops a second click while one is in flight", async () => {
    const { result } = setup();
    listBranchesReturns([local("main")]);
    await load(result, "/a");

    let release: (o: FastForwardOutcome) => void = () => {};
    vi.mocked(api.fastForwardBranch).mockReturnValue(
      new Promise<FastForwardOutcome>((r) => (release = r)),
    );
    let second: unknown;
    await act(async () => {
      const first = result.current.fastForward("main");
      // The daemon would answer the second with `up_to_date` — a refusal for a
      // click that did exactly what was asked is a lie about the repository.
      second = await result.current.fastForward("main");
      release(ffDone([local("main")]));
      await first;
    });
    expect(second).toBeNull();
    expect(api.fastForwardBranch).toHaveBeenCalledTimes(1);
  });

  it("fast-forwards nothing when no repo is loaded", async () => {
    const { result } = setup();
    await act(async () => {
      expect(await result.current.fastForward("main")).toBeNull();
    });
    expect(api.fastForwardBranch).not.toHaveBeenCalled();
  });
});
