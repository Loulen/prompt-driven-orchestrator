// #930 — an Operation an extension refused answers 403 with the body every
// daemon refusal carries (ADR-0035 §3): the slug in `error`, the sentence in
// `message`, the extension's `reason`. The client needs no extension-specific
// code: the thrown ApiError's message is that sentence, so every gesture's
// existing error shows the reason.
import { afterEach, describe, expect, it, vi } from "vitest";

import { ApiError, request } from "./client";

afterEach(() => {
  vi.unstubAllGlobals();
});

const REFUSAL = {
  error: "operation_refused",
  message: "operation refused: renames are frozen",
  reason: "renames are frozen",
};

describe("an Operation refused by an extension", () => {
  it("throws an ApiError whose message carries the extension's reason", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: false, status: 403, json: async () => REFUSAL })),
    );

    const err: unknown = await request("PATCH", "/projects/p1", { body: { name: "x" } }).catch(
      (e: unknown) => e,
    );

    expect(err).toBeInstanceOf(ApiError);
    if (!(err instanceof ApiError)) return;
    expect(err.status).toBe(403);
    expect(err.message).toBe("operation refused: renames are frozen");
    expect(err.body).toEqual(REFUSAL);
  });
});
