// #211 / #206 — a mid-run mutation rejected by the daemon (409) must surface
// the *reason* to the user, not just "mutation rejected". The daemon returns
// { error, rejections: [{ node_id, reason }] }; the thrown save error's
// message must include each rejection reason.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  ApiError,
  deletePipeline,
  duplicateLibraryPipeline,
  fetchPipeline,
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
describe("scope-qualified pipeline ops", () => {
  it("appends ?scope=library to DELETE for a library entry", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await deletePipeline("simple-bugfix", "library");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/pipelines/simple-bugfix?scope=library");
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

  it("appends ?scope=library to GET when opening a library entry", async () => {
    const fetchMock = captureFetch(200, { id: "x", scope: "library" });
    await fetchPipeline("simple-bugfix", "library");
    expect(fetchMock.mock.calls[0][0]).toBe("/pipelines/simple-bugfix?scope=library");
  });

  it("appends ?scope=library to PUT when saving a library entry", async () => {
    const fetchMock = captureFetch(200, { ok: true });
    await savePipeline("simple-bugfix", "name: x", {}, "library");
    const [url, init] = fetchMock.mock.calls[0];
    expect(url).toBe("/pipelines/simple-bugfix?scope=library");
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
    expect(init?.headers).toBeUndefined();
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
