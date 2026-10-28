// #211 / #206 — a mid-run mutation rejected by the daemon (409) must surface
// the *reason* to the user, not just "mutation rejected". The daemon returns
// { error, rejections: [{ node_id, reason }] }; the thrown save error's
// message must include each rejection reason.
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, request, uploadTimeoutMs } from "./shared/http/client";
import { createRun, editRunRepos } from "./run/api/createRun";
import { saveRunPipeline } from "./run/api/runPipeline";
import { deletePipeline, fetchPipeline, savePipeline } from "./pipeline/api/pipelines";
import { duplicateLibraryPipeline } from "./pipeline/api/libraryPipelines";
import { browseFs } from "./workspace/api/browse";

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
// #839 — an upload that fails BETWEEN the browser and the daemon (a reverse
// proxy's body limit, a cut connection, a stalled link) must name that layer,
// the host, the payload and the knobs — never the bare `POST /runs failed: 413`
// or the browser's `TypeError: Failed to fetch`.
describe("upload failures between the browser and the daemon (#839)", () => {
  const attachedRun = () => ({
    pipeline: "p",
    input: "hello",
    variables: {},
    target_repo: "/repo",
    files: [new File([new Uint8Array(1_500_000)], "copilote.html", { type: "text/html" })],
  });

  it("a 413 whose body is not JSON is attributed to the proxy, with the size and both limits", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: false,
        status: 413,
        json: async () => {
          throw new SyntaxError("Unexpected token <");
        },
        text: async () => "<html><body>413 Request Entity Too Large</body></html>",
      })),
    );
    const err = (await createRun(attachedRun()).catch((e) => e)) as ApiError;
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBe(413);
    expect(err.message).toContain("server in front of the daemon");
    expect(err.message).toContain("1.4 MB");
    expect(err.message).toContain("client_max_body_size");
    expect(err.message).toContain("max_attachments_mb");
    expect(err.message).not.toMatch(/^POST \/runs failed: 413$/);
  });

  it("the daemon's own 413 (JSON body) keeps the daemon's sentence", async () => {
    stubFetchWith(413, {
      error: "attachments exceed the per-run limit of 50 MB (at `big.bin`); raise `max_attachments_mb` in Settings or attach less",
    });
    const err = (await createRun(attachedRun()).catch((e) => e)) as ApiError;
    expect(err.message).toMatch(/^attachments exceed the per-run limit/);
    expect(err.message).not.toContain("server in front of the daemon");
  });

  it("a non-JSON error on any status says the reply did not come from the daemon", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({
        ok: false,
        status: 502,
        json: async () => {
          throw new SyntaxError("Unexpected token <");
        },
        text: async () => "<html>Bad Gateway</html>",
      })),
    );
    const err = (await request("GET", "/runs").catch((e) => e)) as ApiError;
    expect(err.status).toBe(502);
    expect(err.message).toContain("GET /runs failed: 502");
    expect(err.message).toContain("intermediary");
    expect(err.body).toBeNull();
  });

  it("a `Failed to fetch` on POST /runs with attachments names the host, the size and the hints", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("Failed to fetch");
      }),
    );
    const err = (await createRun(attachedRun()).catch((e) => e)) as ApiError;
    expect(err).toBeInstanceOf(ApiError);
    expect(err.status).toBeUndefined();
    expect(err.message).toContain("Upload interrupted");
    expect(err.message).toContain("1.4 MB");
    expect(err.message).toContain(window.location.host);
    expect(err.message).toContain("Failed to fetch");
    expect(err.message).toContain("client_max_body_size");
  });

  it("a `Failed to fetch` without attachments stays short and names the host", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        throw new TypeError("Failed to fetch");
      }),
    );
    const err = (await request("GET", "/runs").catch((e) => e)) as ApiError;
    expect(err.message).toBe(
      `GET /runs failed: could not reach ${window.location.host} (Failed to fetch).`,
    );
  });

  it("an upload that stops progressing expires with a timeout sentence", async () => {
    vi.useFakeTimers();
    try {
      // A fetch that only settles when its signal aborts — a stalled upload.
      vi.stubGlobal(
        "fetch",
        vi.fn(
          (_url: string, init?: RequestInit) =>
            new Promise((_resolve, reject) => {
              init?.signal?.addEventListener("abort", () =>
                reject(new DOMException("The operation was aborted.", "AbortError")),
              );
            }),
        ),
      );
      const pending = createRun(attachedRun()).catch((e) => e);
      // 1.5 MB → 2 MB rounded up → 60 s + 20 s.
      expect(uploadTimeoutMs(1_500_000)).toBe(80_000);
      await vi.advanceTimersByTimeAsync(80_000);
      const err = (await pending) as ApiError;
      expect(err).toBeInstanceOf(ApiError);
      expect(err.message).toContain("Upload timed out after 80s");
      expect(err.message).toContain("1.4 MB");
      expect(err.message).toContain("max_attachments_mb");
    } finally {
      vi.useRealTimers();
    }
  });

  it("only uploads get an abort signal — a plain JSON request is untouched", async () => {
    const fn = captureFetch(200, { ok: true });
    await request("GET", "/runs");
    expect(fn.mock.calls[0][1]?.signal).toBeUndefined();
  });
});

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
