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
  sentence: "You can't rename Projects. Ask an administrator.",
};

describe("an Operation refused by an extension", () => {
  it("throws an ApiError whose message is the extension's sentence for a person, the reason kept in its body (CORE-205)", async () => {
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
    expect(err.message).toBe("You can't rename Projects. Ask an administrator.");
    expect(err.body).toEqual(REFUSAL);
  });

  it("without a sentence from the extension, says the core's generic one, never the reason", async () => {
    const bare = { error: REFUSAL.error, message: REFUSAL.message, reason: REFUSAL.reason };
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => ({ ok: false, status: 403, json: async () => bare })),
    );
    const err: unknown = await request("PATCH", "/projects/p1", { body: { name: "x" } }).catch((e: unknown) => e);
    expect(err).toBeInstanceOf(ApiError);
    expect((err as ApiError).message).toBe("You don't have the rights to do this.");
  });
});
