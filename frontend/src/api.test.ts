// #211 / #206 — a mid-run mutation rejected by the daemon (409) must surface
// the *reason* to the user, not just "mutation rejected". The daemon returns
// { error, rejections: [{ node_id, reason }] }; the thrown save error's
// message must include each rejection reason.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ApiError,
  browseFs,
  deletePipeline,
  duplicateLibraryPipeline,
  editRunRepos,
  fetchPipeline,
  markNodeDone,
  request,
  savePipeline,
  saveRunPipeline,
} from "./api";

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

describe("saveRunPipeline rejection surfacing", () => {
  it("includes each rejection reason in the thrown message", async () => {
    stubFetchWith(409, {
      error: "mutation rejected",
      rejections: [
        {
          node_id: "worker",
          reason:
            "cannot change type of node 'worker': its session is live (status 'running'); a running node is immutable, including its type",
        },
      ],
    });

    await expect(saveRunPipeline("r1", "name: x", {})).rejects.toMatchObject({
      status: 409,
      message: expect.stringContaining("cannot change type of node 'worker'"),
    });
  });

  it("keeps the plain error message when there are no rejections", async () => {
    stubFetchWith(400, { error: "invalid YAML: boom" });

    await expect(saveRunPipeline("r1", "name: x", {})).rejects.toMatchObject({
      status: 400,
      message: "invalid YAML: boom",
    });
  });
});

// #216 — pipeline open/save/delete must carry the entry's scope so a `library`
// (or `user`) id colliding with a same-named repo pipeline routes to the
// intended store, not the repo file. The query string is the wire contract the
// daemon branches on.
describe("instance pipeline ops", () => {
  it("ignores obsolete scopes on DELETE", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await deletePipeline("simple-bugfix", "library");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/pipelines/simple-bugfix");
    expect(init).toMatchObject({ method: "DELETE" });
  });

  it("omits the scope query when no scope is given (back-compat)", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await deletePipeline("simple-bugfix");
    expect(fetchMock.mock.calls[0][0]).toBe("/pipelines/simple-bugfix");
  });

  it("does not forward the synthetic 'run' scope as a query", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await deletePipeline("r", "run");
    expect(fetchMock.mock.calls[0][0]).toBe("/pipelines/r");
  });

  it("ignores obsolete scopes on GET", async () => {
    const fetchMock = captureFetch(200, { id: "x", scope: "library" });
    await fetchPipeline("simple-bugfix", "library");
    expect(fetchMock.mock.calls[0][0]).toBe("/pipelines/simple-bugfix");
  });

  it("ignores obsolete scopes on PUT", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await savePipeline("simple-bugfix", "name: x", {}, "library");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/pipelines/simple-bugfix");
    expect(init).toMatchObject({ method: "PUT" });
  });
});

// #224 — duplicate a library pipeline template via POST .../duplicate.
describe("duplicateLibraryPipeline", () => {
  it("POSTs to /library/pipelines/{id}/duplicate and returns the body", async () => {
    const fetchMock = captureFetch(201, {
      id: "fixture-copy",
      scope: "user",
      entry: { id: "fixture-copy", name: "fixture (copy)" },
    });
    const result = await duplicateLibraryPipeline("fixture");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/library/pipelines/fixture/duplicate");
    expect(init).toMatchObject({ method: "POST" });
    expect(result).toMatchObject({ id: "fixture-copy", scope: "user" });
  });

  it("encodes the id in the URL", async () => {
    const fetchMock = captureFetch(201, { id: "x", scope: "user", entry: null });
    await duplicateLibraryPipeline("a b/c");
    expect(fetchMock.mock.calls[0][0]).toBe("/library/pipelines/a%20b%2Fc/duplicate");
  });

  it("throws on a non-2xx response", async () => {
    captureFetch(404, "pipeline template not found");
    await expect(duplicateLibraryPipeline("ghost")).rejects.toThrow(/404/);
  });
});

// #358 — the whole client funnels through one `request()` core with one error
// contract (`ApiError`). Test the seam once here instead of per endpoint.
describe("request core", () => {
  it("resolves the parsed JSON body on success (json mode default)", async () => {
    stubFetchWith(200, { id: "x" });
    await expect(request("GET", "/x")).resolves.toMatchObject({ id: "x" });
  });

  it("throws an ApiError carrying status + message on an HTTP error with a body", async () => {
    stubFetchWith(400, { error: "invalid YAML: boom" });
    const err = await request("GET", "/x").catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 400, message: "invalid YAML: boom" });
  });

  it("lifts `line` and folds `rejections[].reason` into a structured error", async () => {
    stubFetchWith(409, {
      error: "mutation rejected",
      line: 12,
      rejections: [{ reason: "node running" }],
    });
    const err = await request("PUT", "/pipelines/p").catch((e) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect(err).toMatchObject({ status: 409, line: 12 });
    expect((err as ApiError).message).toContain("node running");
  });

  it("falls back to `<label> failed: <status>` when the body has no message", async () => {
    stubFetchWith(500, null);
    await expect(request("GET", "/x", { label: "boom" })).rejects.toMatchObject({
      status: 500,
      message: "boom failed: 500",
    });
  });

  it("preserves the raw error body on the ApiError", async () => {
    stubFetchWith(400, { error: "bad", extra: 1 });
    const err = (await request("GET", "/x").catch((e) => e)) as ApiError;
    expect(err.body).toMatchObject({ error: "bad", extra: 1 });
  });

  // D1 — the contract MUST subclass Error, else the ~7 UI callers that render
  // via `err instanceof Error ? err.message` fall back to `[object Object]`.
  it("ApiError is an instanceof Error", () => {
    expect(new ApiError("x") instanceof Error).toBe(true);
  });

  it("serializes an object body as JSON with a Content-Type header", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await request("POST", "/things", { body: { a: 1 } });
    const [, init] = fetchMock.mock.calls[0];
    expect(init).toMatchObject({
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ a: 1 }),
    });
  });

  it("sends a FormData body without a manual Content-Type (browser sets the boundary)", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    const form = new FormData();
    form.append("k", "v");
    await request("POST", "/upload", { body: form });
    const [, init] = fetchMock.mock.calls[0];
    expect(init?.body).toBeInstanceOf(FormData);
    // #507: the origin hint rides on every request, but Content-Type must stay
    // absent so the browser sets the multipart boundary itself.
    expect(init?.headers).toEqual({ "X-PDO-Actor": "ui" });
  });

  it("declares X-PDO-Actor: ui on every request, even a body-less GET (#507)", async () => {
    const fetchMock = captureFetch(200, { id: "x" });
    await request("GET", "/things");
    const [, init] = fetchMock.mock.calls[0];
    expect((init?.headers as Record<string, string>)["X-PDO-Actor"]).toBe("ui");
    // No body → no Content-Type.
    expect((init?.headers as Record<string, string>)["Content-Type"]).toBeUndefined();
  });

  it("appends query params, encoding values and dropping undefined", async () => {
    const fetchMock = captureFetch(200, []);
    await request("GET", "/search", { query: { q: "a b", n: 3, skip: undefined } });
    expect(fetchMock.mock.calls[0][0]).toBe("/search?q=a%20b&n=3");
  });

  it("joins query with & when the path already has a query string", async () => {
    const fetchMock = captureFetch(200, []);
    await request("GET", "/search?scope=x", { query: { q: "y" } });
    expect(fetchMock.mock.calls[0][0]).toBe("/search?scope=x&q=y");
  });

  it("returns the raw string in text mode without JSON-parsing", async () => {
    stubFetchWith(200, "plain text body");
    await expect(request("GET", "/artifact", { responseMode: "text" })).resolves.toBe(
      "plain text body",
    );
  });

  it("resolves undefined in void mode", async () => {
    stubFetchWith(200, { ignored: true });
    await expect(request("POST", "/cmd", { responseMode: "void" })).resolves.toBeUndefined();
  });

  it("returns the Response itself in raw mode and never throws on a non-ok status", async () => {
    stubFetchWith(409, { conflict: true });
    const resp = await request<Response>("DELETE", "/pipelines/p", { responseMode: "raw" });
    expect(resp.status).toBe(409);
    expect(await resp.json()).toMatchObject({ conflict: true });
  });
});

// #431 — `browseRepos` → `browseFs` (`/repos/browse` → `/fs/browse`), plus two
// optional flags. The wire contract IS the AC: with no options the URL must be
// EXACTLY `/fs/browse`, query-string-free, so the daemon takes the byte-identical
// pre-#431 default branch.
describe("browseFs wire contract", () => {
  const EMPTY = { path: "/", parent: null, entries: [], truncated: false, error: null };

  it("hits /fs/browse with NO query string when called with no argument", async () => {
    const fetchMock = captureFetch(200, EMPTY);
    await browseFs();
    expect(fetchMock.mock.calls[0][0]).toBe("/fs/browse");
  });

  it("encodes the path", async () => {
    const fetchMock = captureFetch(200, EMPTY);
    await browseFs("/a b");
    expect(fetchMock.mock.calls[0][0]).toBe("/fs/browse?path=%2Fa%20b");
  });

  it("sends both flags as the lowercase literals the daemon accepts", async () => {
    const fetchMock = captureFetch(200, EMPTY);
    await browseFs("/x", { files: true, hidden: true });
    expect(fetchMock.mock.calls[0][0]).toBe("/fs/browse?path=%2Fx&files=true&hidden=true");
  });

  it("keeps a false flag OFF the wire (no redundant explicit default)", async () => {
    const fetchMock = captureFetch(200, EMPTY);
    await browseFs("/x", { files: false, hidden: true });
    expect(fetchMock.mock.calls[0][0]).toBe("/fs/browse?path=%2Fx&hidden=true");
  });

  it("omits path when only flags are given", async () => {
    const fetchMock = captureFetch(200, EMPTY);
    await browseFs(undefined, { files: true });
    expect(fetchMock.mock.calls[0][0]).toBe("/fs/browse?files=true");
  });
});

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

// #465 slice 2 / ADR-0042 — mid-run edit of a Run's read-only secondary list.
describe("editRunRepos outcome union (#465)", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("PATCHes /runs/{id}/repos with the add/remove body", async () => {
    const fetchMock = captureFetch(200, { run_id: "r1", target_repos: [] });
    await editRunRepos("r1", { add: [{ repo: "/repos/lib" }], remove: ["old"] });
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toContain("/runs/r1/repos");
    expect(init?.method).toBe("PATCH");
    expect(JSON.parse(init?.body as string)).toEqual({
      add: [{ repo: "/repos/lib" }],
      remove: ["old"],
    });
  });

  it("returns the reprojected run on 200", async () => {
    stubFetchWith(200, { run_id: "r1", target_repos: [{ alias: "lib" }] });
    const out = await editRunRepos("r1", { add: [{ repo: "/repos/lib" }] });
    if (out.kind !== "ok") throw new Error("expected ok");
    expect(out.run.run_id).toBe("r1");
  });

  it("surfaces a typed refusal as a verdict, not a thrown error", async () => {
    stubFetchWith(409, {
      error: "run_not_editable",
      message: "run is completed and its repository list is frozen",
      status: "completed",
    });
    const out = await editRunRepos("r1", { remove: ["lib"] });
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.slug).toBe("run_not_editable");
    expect(out.status).toBe(409);
    expect(out.message).toContain("frozen");
  });

  it("carries the daemon's message even when the slug is unknown", async () => {
    stubFetchWith(400, { error: "bad_secondary_repo", message: "not a git repository" });
    const out = await editRunRepos("r1", { add: [{ repo: "/nope" }] });
    if (out.kind !== "refused") throw new Error("expected a refusal");
    expect(out.slug).toBe("bad_secondary_repo");
    expect(out.message).toContain("not a git repository");
  });
});
