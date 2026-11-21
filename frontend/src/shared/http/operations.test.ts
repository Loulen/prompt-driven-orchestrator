// The Opérations en cours: every mutation the client sends is followed from
// its start to its outcome; reads and silent requests are not.
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { request } from "./client";
import {
  beginOperation,
  errorAlreadyShown,
  failuresSaidByCaller,
  fallbackLabel,
  resetOperations,
  trackOperation,
  useOperationsStore,
} from "./operations";
import { resetToasts, useToasts } from "../ui/toast";

const ops = () => useOperationsStore.getState().operations;
const toasts = () => useToasts.getState().toasts;

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.useFakeTimers();
  resetOperations();
  resetToasts();
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("an operation", () => {
  it("is counted by the status bar only once it has lasted 300 ms", () => {
    const end = beginOperation("Archiving run");
    expect(ops()).toHaveLength(1);
    expect(ops()[0].visible).toBe(false);
    vi.advanceTimersByTime(299);
    expect(ops()[0].visible).toBe(false);
    vi.advanceTimersByTime(1);
    expect(ops()[0].visible).toBe(true);
    end();
    expect(ops()).toHaveLength(0);
  });

  it("counts each operation in flight", () => {
    const a = beginOperation("Archiving run");
    const b = beginOperation("Approving node");
    expect(ops().map((o) => o.label)).toEqual(["Archiving run", "Approving node"]);
    a();
    expect(ops().map((o) => o.label)).toEqual(["Approving node"]);
    b();
  });

  it("a quick success says nothing", () => {
    const end = beginOperation("Saving pipeline", { done: "Pipeline saved" });
    vi.advanceTimersByTime(1500);
    end();
    expect(toasts()).toHaveLength(0);
  });

  it("a success past 2 s says so in a toast, in the past tense, with its duration", () => {
    const end = beginOperation("Saving pipeline", { done: "Pipeline saved" });
    vi.advanceTimersByTime(3000);
    end();
    expect(toasts()).toHaveLength(1);
    expect(toasts()[0]).toMatchObject({ kind: "success", message: "Pipeline saved (3 s)" });
  });

  it("without a past tense, the success toast says « Done: <label> »", () => {
    const end = beginOperation("Syncing prices…");
    vi.advanceTimersByTime(2500);
    end();
    expect(toasts()[0].message).toBe("Done: Syncing prices (3 s)");
  });

  it("a failure always says the daemon's message, even a quick one", () => {
    const end = beginOperation("Archiving run");
    const error = new Error("worktree is locked by another git process");
    end(error);
    expect(toasts()).toHaveLength(1);
    expect(toasts()[0]).toMatchObject({
      kind: "error",
      title: "Archiving run failed",
      message: "worktree is locked by another git process",
    });
    expect(errorAlreadyShown(error)).toBe(true);
    expect(errorAlreadyShown(new Error("other"))).toBe(false);
  });

  it("ending twice changes nothing more", () => {
    const end = beginOperation("Archiving run");
    end(new Error("x"));
    end(new Error("y"));
    expect(toasts()).toHaveLength(1);
  });
});

describe("the label of a mutation that names none", () => {
  it("follows its method", () => {
    expect(fallbackLabel("POST")).toBe("Saving…");
    expect(fallbackLabel("PUT")).toBe("Updating…");
    expect(fallbackLabel("PATCH")).toBe("Updating…");
    expect(fallbackLabel("DELETE")).toBe("Deleting…");
  });
});

describe("trackOperation, the extensions' way to name their mutations", () => {
  it("follows the work until it settles and hands back its result", async () => {
    const work = deferred<number>();
    const tracked = trackOperation("Moving note", work.promise, { icon: "/ext/test/icon.svg" });
    expect(ops()[0]).toMatchObject({ label: "Moving note", icon: "/ext/test/icon.svg" });
    work.resolve(42);
    await expect(tracked).resolves.toBe(42);
    expect(ops()).toHaveLength(0);
  });

  it("hands back the rejection unchanged, after its toast", async () => {
    const boom = new Error("note is locked");
    await expect(trackOperation("Moving note", () => Promise.reject(boom))).rejects.toBe(boom);
    expect(toasts()[0]).toMatchObject({ kind: "error", title: "Moving note failed", message: "note is locked" });
  });
});

describe("the HTTP client", () => {
  function stubFetch(answer: () => Promise<unknown>) {
    const fetch = vi.fn(answer);
    vi.stubGlobal("fetch", fetch);
    return fetch;
  }
  const ok = (body: unknown = {}) => ({ ok: true, status: 200, json: async () => body, text: async () => "" });

  it("follows a mutation under its label until the daemon answers", async () => {
    const answer = deferred<unknown>();
    stubFetch(() => answer.promise);
    const pending = request("POST", "/runs/r1/archive", { label: "Archiving run", responseMode: "void" });
    expect(ops().map((o) => o.label)).toEqual(["Archiving run"]);
    answer.resolve(ok());
    await pending;
    expect(ops()).toHaveLength(0);
  });

  it("names a mutation without a label after its method", async () => {
    const answer = deferred<unknown>();
    stubFetch(() => answer.promise);
    const pending = request("DELETE", "/things/1", { responseMode: "void" });
    expect(ops()[0].label).toBe("Deleting…");
    answer.resolve(ok());
    await pending;
  });

  it("never follows a read", async () => {
    const answer = deferred<unknown>();
    stubFetch(() => answer.promise);
    const pending = request("GET", "/runs");
    expect(ops()).toHaveLength(0);
    answer.resolve(ok([]));
    await pending;
  });

  it("never follows a silent mutation nor one that outlives the page", async () => {
    stubFetch(async () => ok());
    const a = request("POST", "/authorize", { silent: true });
    const b = request("DELETE", "/sessions/x", { keepalive: true, responseMode: "void" });
    expect(ops()).toHaveLength(0);
    await Promise.all([a, b]);
    expect(toasts()).toHaveLength(0);
  });

  it("a refused mutation shows the daemon's message in a toast and still throws", async () => {
    stubFetch(async () => ({ ok: false, status: 400, json: async () => ({ error: "invalid pipeline: no start" }) }));
    const err = await request("PUT", "/pipelines/p", { label: "Saving pipeline", body: {} }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(Error);
    expect(toasts()[0]).toMatchObject({ kind: "error", title: "Saving pipeline failed", message: "invalid pipeline: no start" });
    expect(errorAlreadyShown(err)).toBe(true);
  });

  it("a refusal without daemon body does not repeat its label", async () => {
    stubFetch(async () => ({ ok: false, status: 502, json: async () => ({}) }));
    await request("POST", "/x", { label: "Archiving run" }).catch(() => {});
    expect(toasts()[0].message).toBe("502");
  });

  it("a raw mutation the daemon broke down on (5xx) is toasted", async () => {
    const resp = { ok: false, status: 500, clone: () => ({ json: async () => ({ error: "disk full" }) }) };
    stubFetch(async () => resp);
    await request("DELETE", "/pipelines/p", { label: "Deleting pipeline", responseMode: "raw" });
    expect(toasts()[0]).toMatchObject({ kind: "error", title: "Deleting pipeline failed", message: "disk full" });
  });

  it("a refusal the caller says itself (quietOn) ends without a toast, and still throws", async () => {
    stubFetch(async () => ({ ok: false, status: 404, json: async () => ({ error: "no such run" }) }));
    const err = await request("PUT", "/runs/r/pipeline", { label: "Saving run pipeline", quietOn: [404] }).catch(
      (e: unknown) => e,
    );
    expect(err).toBeInstanceOf(Error);
    expect(ops()).toHaveLength(0);
    expect(toasts()).toHaveLength(0);
  });

  it("a raw mutation the daemon refused is its caller's to say: no toast", async () => {
    const resp = { ok: false, status: 409, json: async () => ({ error: "busy" }) };
    stubFetch(async () => resp);
    const got = await request("POST", "/x/commands", { label: "Approving node", responseMode: "raw" });
    expect(got).toBe(resp);
    expect(ops()).toHaveLength(0);
    expect(toasts()).toHaveLength(0);
  });
});

describe("a caller that says the failure itself", () => {
  it("awaits what it started: the operation shows, its failure is no toast but marked as said", async () => {
    const boom = new Error("worktree is locked");
    const work = deferred<void>();
    const outcome = failuresSaidByCaller(() => trackOperation("Archiving run", work.promise));
    expect(ops().map((o) => o.label)).toEqual(["Archiving run"]);
    work.reject(boom);
    await expect(outcome).rejects.toBe(boom);
    expect(ops()).toHaveLength(0);
    expect(toasts()).toHaveLength(0);
    expect(errorAlreadyShown(boom)).toBe(true);
  });

  it("still says a success past 2 s", async () => {
    const work = deferred<void>();
    const outcome = failuresSaidByCaller(() => trackOperation("Archiving run", work.promise, { done: "Run archived" }));
    vi.advanceTimersByTime(2500);
    work.resolve();
    await outcome;
    expect(toasts()[0]).toMatchObject({ kind: "success", message: "Run archived (3 s)" });
  });

  it("hands back nothing to await: the failure is toasted as usual", async () => {
    const boom = new Error("worktree is locked");
    const work = deferred<void>();
    let started: Promise<void> | undefined;
    failuresSaidByCaller(() => {
      started = trackOperation("Archiving run", work.promise);
    });
    work.reject(boom);
    await started!.catch(() => {});
    expect(toasts()[0]).toMatchObject({ kind: "error", title: "Archiving run failed", message: "worktree is locked" });
  });

  it("an operation begun outside it keeps its toast", async () => {
    const outside = deferred<void>();
    const before = trackOperation("Saving pipeline", outside.promise);
    const inside = deferred<void>();
    const awaited = failuresSaidByCaller(() => trackOperation("Archiving run", inside.promise));
    outside.reject(new Error("invalid"));
    inside.reject(new Error("locked"));
    await before.catch(() => {});
    await awaited.catch(() => {});
    expect(toasts().map((t) => t.title)).toEqual(["Saving pipeline failed"]);
  });

  it("a caller that catches the failure and resolves said nothing: the held toast shows", async () => {
    const work = deferred<void>();
    const outcome = failuresSaidByCaller(() => trackOperation("Retrying run", work.promise).catch(() => undefined));
    work.reject(new Error("run is live"));
    await outcome;
    expect(toasts()[0]).toMatchObject({ kind: "error", title: "Retrying run failed", message: "run is live" });
  });
});

