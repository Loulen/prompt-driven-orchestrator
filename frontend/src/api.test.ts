// #211 / #206 — a mid-run mutation rejected by the daemon (409) must surface
// the *reason* to the user, not just "mutation rejected". The daemon returns
// { error, rejections: [{ node_id, reason }] }; the thrown save error's
// message must include each rejection reason.
import { afterEach, describe, expect, it, vi } from "vitest";

import { saveRunPipeline } from "./api";

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
    })),
  );
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
