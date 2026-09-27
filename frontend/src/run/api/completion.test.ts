import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError } from "../../shared/http/client";
import { markNodeDone, releaseNodeCompletion } from "./completion";

afterEach(() => {
  vi.unstubAllGlobals();
});

function stubFetchWith(status: number, body: unknown) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({
      ok: status >= 200 && status < 300,
      status,
      json: async () => body,
      text: async () => (typeof body === "string" ? body : JSON.stringify(body)),
    })),
  );
}

type FetchLike = (
  url: string,
  init?: RequestInit,
) => Promise<{
  ok: boolean;
  status: number;
  json: () => Promise<unknown>;
  text: () => Promise<string>;
}>;

/** Stub fetch and return the mock so callers can inspect the request URL. */
function captureFetch(status: number, body: unknown) {
  const fn = vi.fn<FetchLike>(async () => ({
    ok: status >= 200 && status < 300,
    status,
    json: async () => body,
    text: async () => (typeof body === "string" ? body : JSON.stringify(body)),
  }));
  vi.stubGlobal("fetch", fn);
  return fn;
}

// #490 / ADR-0035 — `markNodeDone` had ZERO tests before this issue, and it is the
// only place a refusal can still fail silently: vitest does not run in CI (the
// frontend job is install / typecheck / lint / build), so this file is guarded by
// `make test` alone. One case per branch of the outcome union.
describe("markNodeDone outcome union (#490)", () => {
  /** A stub whose body is NOT valid JSON — the shape `POST …/done` answers on success. */
  function stubNonJson(status: number, text: string) {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: status >= 200 && status < 300,
        status,
        json: async () => {
          throw new SyntaxError("Unexpected token");
        },
        text: async () => text,
      })),
    );
  }

  it("reads a plain 200 as completed", async () => {
    stubFetchWith(200, { ok: true });
    await expect(markNodeDone("r1", "n1", 1)).resolves.toEqual({ kind: "completed" });
  });

  describe("releaseNodeCompletion (#764)", () => {
    it("posts the displayed iteration to the release route", async () => {
      const fetchMock = captureFetch(200, { ok: true, released: true });
      await expect(releaseNodeCompletion("r1", "n1", 3)).resolves.toEqual({
        kind: "released",
      });
      const [url, init] = fetchMock.mock.calls[0];
      expect(url).toContain("/runs/r1/nodes/n1/release");
      expect(JSON.parse(init?.body as string)).toEqual({ iter: 3 });
    });

    it("returns a named 409 refusal as a verdict", async () => {
      stubFetchWith(409, {
        error: "node_session_not_live",
        recoverable: true,
        message: "no live session",
      });
      await expect(releaseNodeCompletion("r1", "n1", 1)).resolves.toEqual({
        kind: "refused",
        slug: "node_session_not_live",
        recoverable: true,
        message: "no live session",
      });
    });
  });

  it("reads a 200 with noop:true as a legal duplicate, not a refusal", async () => {
    stubFetchWith(200, { ok: true, noop: true, reason: "already completed" });
    await expect(markNodeDone("r1", "n1", 1)).resolves.toEqual({
      kind: "noop",
      reason: "already completed",
    });
  });

  it("does not throw a bare SyntaxError on a non-JSON success body", async () => {
    // Pre-#490 this line was `await resp.json()` unguarded, which violated the
    // module's single-error-type contract.
    stubNonJson(200, "ok");
    await expect(markNodeDone("r1", "n1", 1)).resolves.toEqual({ kind: "completed" });
  });

  it("reads a 409 missing_outputs as a refusal that is still your turn", async () => {
    stubFetchWith(409, {
      error: "missing_outputs",
      recoverable: true,
      missing: ["review", "notes"],
    });
    const out = await markNodeDone("r1", "n1", 1);
    expect(out.kind).toBe("refused");
    if (out.kind !== "refused") throw new Error("unreachable");
    expect(out.slug).toBe("missing_outputs");
    expect(out.recoverable).toBe(true);
    expect(out.missing).toEqual(["review", "notes"]);
  });

  it("reads a 409 frontmatter_retry_exhausted as a terminal refusal with its violations", async () => {
    stubFetchWith(409, {
      error: "frontmatter_retry_exhausted",
      recoverable: false,
      violations: [{ port: "review", field: "verdict", reason: "not in enum" }],
    });
    const out = await markNodeDone("r1", "n1", 1);
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.slug).toBe("frontmatter_retry_exhausted");
    expect(out.recoverable).toBe(false);
    expect(out.violations).toEqual([
      { port: "review", field: "verdict", reason: "not in enum" },
    ]);
  });

  it("surfaces the transition guard's prose instead of an empty missing list", async () => {
    // THE regression of #490: this refusal used to be read as `missing_outputs` with
    // `missing: []`, and the banner was gated on `length > 0` — so the most frequent
    // refusal of all displayed nothing at all.
    stubFetchWith(409, {
      error: "completion_rejected",
      recoverable: false,
      message: "run r1 is Failed: resume the run first",
    });
    const out = await markNodeDone("r1", "n1", 1);
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.slug).toBe("completion_rejected");
    expect(out.message).toContain("resume the run first");
    expect(out.missing).toEqual([]);
  });

  it("reads the nested `detail` of a script fail-fast", async () => {
    stubFetchWith(409, {
      error: "script_validation_failed",
      recoverable: false,
      detail: { kind: "missing_outputs", missing: ["out"] },
    });
    const out = await markNodeDone("r1", "n1", 1);
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.missing).toEqual(["out"]);
  });

  it("renders an unknown slug verbatim rather than hiding it (ADR-0001)", async () => {
    stubFetchWith(409, { error: "some_future_cause", recoverable: false, message: "nope" });
    const out = await markNodeDone("r1", "n1", 1);
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.slug).toBeNull();
    expect(out.message).toBe("nope");
  });

  it("leaves `recoverable` null when the daemon sent no flag", async () => {
    stubFetchWith(409, { error: "missing_outputs", missing: [] });
    const out = await markNodeDone("r1", "n1", 1);
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.recoverable).toBeNull();
  });

  it("throws on a 410, a 404 and a 5xx — those are breakdowns, not verdicts", async () => {
    for (const status of [404, 410, 500]) {
      stubFetchWith(status, { error: "boom" });
      await expect(markNodeDone("r1", "n1", 1)).rejects.toBeInstanceOf(ApiError);
      vi.unstubAllGlobals();
    }
  });

  it("never renders a 2xx that still carries a `status` key as a completion", async () => {
    // An older daemon answering a refusal with a success status. Reading that as
    // "completed" IS the bug.
    stubFetchWith(200, { status: "script_validation_failed" });
    const out = await markNodeDone("r1", "n1", 1);
    expect(out.kind).toBe("refused");
  });

  it("posts the mark_node_done command with node_id and iter", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await markNodeDone("r1", "n1", 3);
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toContain("/runs/r1/commands");
    expect(JSON.parse(init?.body as string)).toEqual({
      kind: "mark_node_done",
      node_id: "n1",
      iter: 3,
    });
  });
});
