import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  askAuthorization: vi.fn(),
}));

import { askAuthorization, type OperationQuestion } from "./api";
import { useRefusals } from "./useRefusals";

const asked = vi.mocked(askAuthorization);
const OPERATIONS: OperationQuestion[] = [
  { resource: "agent_profile", verb: "create" },
  { resource: "agent_profile", verb: "update", target: "p1" },
];

beforeEach(() => asked.mockReset());

describe("useRefusals (CORE-205)", () => {
  it("answers each Operation with the sentence of its refusal, nothing before the answer", async () => {
    asked.mockResolvedValue([
      { decision: "deny", reason: "the test rules refuse it", sentence: "You can't change agent profiles. Ask an administrator." },
      { decision: "allow" },
    ]);
    const { result } = renderHook(() => useRefusals(OPERATIONS));
    expect(result.current).toEqual([undefined, undefined]);
    await waitFor(() =>
      expect(result.current).toEqual(["You can't change agent profiles. Ask an administrator.", undefined]),
    );
    expect(asked).toHaveBeenCalledWith(OPERATIONS);
  });

  it("says the generic sentence when the extension gives none, and keeps the last answer on a failure", async () => {
    asked.mockResolvedValueOnce([{ decision: "deny", reason: "the test rules refuse it" }, { decision: "allow" }]);
    const { result } = renderHook(() => useRefusals(OPERATIONS));
    await waitFor(() => expect(result.current[0]).toBe("You don't have the rights to do this."));
    asked.mockRejectedValueOnce(new Error("offline"));
    await act(async () => window.dispatchEvent(new Event("focus")));
    expect(result.current[0]).toBe("You don't have the rights to do this.");
  });

  it("asks nothing while inactive, or for no Operation", () => {
    renderHook(() => useRefusals(OPERATIONS, false));
    renderHook(() => useRefusals([]));
    expect(asked).not.toHaveBeenCalled();
  });
});
