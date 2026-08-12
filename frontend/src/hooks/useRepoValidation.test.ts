import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useRepoValidation } from "./useRepoValidation";
import * as api from "../api";

vi.mock("../api", () => ({
  validateRepo: vi.fn(),
}));

const REPO = "/home/user/project";
/** The debounce the field has always used. */
const DEBOUNCE_MS = 400;

beforeEach(() => {
  vi.mocked(api.validateRepo).mockReset().mockResolvedValue({ valid: true });
  vi.useFakeTimers({ shouldAdvanceTime: true });
});

afterEach(() => {
  vi.useRealTimers();
});

function setup(
  open = true,
  loadBranches = vi.fn<(repoPath: string) => Promise<void>>(async () => {}),
) {
  const clearBranches = vi.fn<() => void>();
  const rendered = renderHook(
    ({ open }: { open: boolean }) => useRepoValidation({ open, loadBranches, clearBranches }),
    { initialProps: { open } },
  );
  return { ...rendered, loadBranches, clearBranches };
}

/** Let the debounce fire and its whole async chain settle. */
async function settle(ms = DEBOUNCE_MS) {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

describe("useRepoValidation — the debounced verdict", () => {
  it("does not validate anything while the dialog is closed", async () => {
    const { result } = setup(false);
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(api.validateRepo).not.toHaveBeenCalled();
  });

  it("does not validate a blank path", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange("   "));
    await settle();
    expect(api.validateRepo).not.toHaveBeenCalled();
  });

  it("waits out the debounce before asking the daemon", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle(DEBOUNCE_MS - 1);
    expect(api.validateRepo).not.toHaveBeenCalled();
    await settle(1);
    expect(api.validateRepo).toHaveBeenCalledTimes(1);
  });

  // Typing is a stream of values; only the one the user stopped on is worth a round-trip.
  it("collapses a burst of keystrokes into one request for the settled value", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange("/home"));
    await settle(100);
    act(() => result.current.handleRepoChange("/home/user"));
    await settle(100);
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(api.validateRepo).toHaveBeenCalledTimes(1);
    expect(api.validateRepo).toHaveBeenCalledWith(REPO);
  });

  it("validates the trimmed path", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange(`  ${REPO}  `));
    await settle();
    expect(api.validateRepo).toHaveBeenCalledWith(REPO);
  });

  it("drops a pending request when the dialog closes", async () => {
    const { result, rerender } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle(100);
    rerender({ open: false });
    await settle();
    expect(api.validateRepo).not.toHaveBeenCalled();
  });
});

describe("useRepoValidation — handing over to the branch loader", () => {
  it("hands a valid repo, trimmed, to the branch loader", async () => {
    const { result, loadBranches, clearBranches } = setup();
    act(() => result.current.handleRepoChange(`  ${REPO}  `));
    await settle();
    expect(result.current.repoValid).toBe(true);
    expect(result.current.repoError).toBeNull();
    expect(loadBranches).toHaveBeenCalledWith(REPO);
    expect(clearBranches).not.toHaveBeenCalled();
  });

  /**
   * The single async chain the modal has always had: branches load INSIDE the validation,
   * so the field stays busy until the list it gates has landed. A branch list arriving
   * after the spinner stopped would let the user launch against a stale branch — the #454
   * family.
   */
  it("keeps the field validating until the branches have landed", async () => {
    let release!: () => void;
    const pending = new Promise<void>((resolve) => {
      release = resolve;
    });
    const { result } = setup(true, vi.fn<(repoPath: string) => Promise<void>>(() => pending));
    act(() => result.current.handleRepoChange(REPO));
    await settle();

    expect(result.current.repoValid).toBe(true);
    expect(result.current.repoValidating).toBe(true);

    await act(async () => {
      release();
      // Flush the microtask queue so the loader's continuation — and its `finally` — run.
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.repoValidating).toBe(false);
  });

  it("rejects an invalid repo with the daemon's reason and drops the branches", async () => {
    vi.mocked(api.validateRepo).mockResolvedValue({ valid: false, error: "not a git repository" });
    const { result, loadBranches, clearBranches } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(result.current.repoValid).toBe(false);
    expect(result.current.repoError).toBe("not a git repository");
    expect(clearBranches).toHaveBeenCalled();
    expect(loadBranches).not.toHaveBeenCalled();
  });

  it("falls back to a generic reason when the daemon names none", async () => {
    vi.mocked(api.validateRepo).mockResolvedValue({ valid: false });
    const { result } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(result.current.repoError).toBe("Not a valid git repository");
  });

  it("reads a failed request as an invalid repo rather than leaving the verdict open", async () => {
    vi.mocked(api.validateRepo).mockRejectedValue(new Error("daemon unreachable"));
    const { result, clearBranches } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(result.current.repoValid).toBe(false);
    expect(result.current.repoError).toBe("Failed to validate repository");
    expect(result.current.repoValidating).toBe(false);
    expect(clearBranches).toHaveBeenCalled();
  });
});

describe("useRepoValidation — clearing the verdict", () => {
  it("clears the verdict and the branches when the field is emptied", async () => {
    const { result, clearBranches } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(result.current.repoValid).toBe(true);

    act(() => result.current.handleRepoChange(""));
    expect(result.current.targetRepo).toBe("");
    expect(result.current.repoValid).toBeNull();
    expect(result.current.repoError).toBeNull();
    expect(clearBranches).toHaveBeenCalled();
  });

  // Retyping over a valid path keeps the old verdict until the new one resolves; only an
  // EMPTY field is an immediate "nothing chosen".
  it("keeps the standing verdict while a non-empty path is being retyped", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    act(() => result.current.handleRepoChange(`${REPO}-b`));
    expect(result.current.repoValid).toBe(true);
  });

  /**
   * #470: `resetRepo` points the field somewhere new AND throws the verdict away. The
   * modal is always-mounted (#386), so a `repoValid === true` surviving a close would
   * otherwise sit next to a repo nobody validated — and `canLaunch` would say yes to the
   * request the daemon answers 400 to.
   */
  it("resetRepo re-points the field and drops the verdict (#470)", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(result.current.repoValid).toBe(true);

    act(() => result.current.resetRepo(""));
    expect(result.current.targetRepo).toBe("");
    expect(result.current.repoValid).toBeNull();
    expect(result.current.repoError).toBeNull();
  });

  it("resetRepo can seed a repo without claiming it is valid", () => {
    const { result } = setup();
    act(() => result.current.resetRepo("/from/a/trigger"));
    expect(result.current.targetRepo).toBe("/from/a/trigger");
    expect(result.current.repoValid).toBeNull();
  });
});

describe("useRepoValidation — the field's border", () => {
  it("stays neutral until there is a verdict", () => {
    const { result } = setup();
    expect(result.current.repoBorderClass).toBe("border-line-strong focus:border-acc");
  });

  it("goes accent on a valid repo and failed on an invalid one", async () => {
    const { result } = setup();
    act(() => result.current.handleRepoChange(REPO));
    await settle();
    expect(result.current.repoBorderClass).toBe("border-acc focus:border-acc");

    vi.mocked(api.validateRepo).mockResolvedValue({ valid: false, error: "nope" });
    act(() => result.current.handleRepoChange("/tmp/not-a-repo"));
    await settle();
    expect(result.current.repoBorderClass).toBe("border-st-failed focus:border-st-failed");
  });
});
