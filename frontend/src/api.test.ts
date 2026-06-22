// #211 / #206 — a mid-run mutation rejected by the daemon (409) must surface
// the *reason* to the user, not just "mutation rejected". The daemon returns
// { error, rejections: [{ node_id, reason }] }; the thrown save error's
// message must include each rejection reason.
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  deletePipeline,
  duplicateLibraryPipeline,
  fetchPipeline,
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
