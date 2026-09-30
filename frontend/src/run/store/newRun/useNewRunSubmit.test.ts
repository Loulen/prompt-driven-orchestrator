import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../../shared/generated/RunState";
import { createdRun } from "./useNewRunSubmit";

vi.mock("../../api/runs", () => ({ fetchRun: vi.fn() }));
const { fetchRun } = await import("../../api/runs");

beforeEach(() => vi.mocked(fetchRun).mockReset());

describe("the Run the run/new sections receive (ADR-0081, #31)", () => {
  it("carries the Attributes the extensions stamped on the create, read back from the daemon", async () => {
    vi.mocked(fetchRun).mockResolvedValue({
      run_id: "r1",
      name: "Nightly",
      attributes: { "test/started_by": "alice" },
    } as unknown as RunState);
    await expect(createdRun("r1", "")).resolves.toEqual({
      kind: "run",
      id: "r1",
      name: "Nightly",
      attributes: { "test/started_by": "alice" },
    });
    expect(fetchRun).toHaveBeenCalledWith("r1");
  });

  it("falls back on the typed name, and on no Attributes when the Run carries none", async () => {
    vi.mocked(fetchRun).mockResolvedValue({ run_id: "r2" } as unknown as RunState);
    await expect(createdRun("r2", "  typed  ")).resolves.toEqual({
      kind: "run",
      id: "r2",
      name: "typed",
      attributes: {},
    });
    await expect(createdRun("r2", "")).resolves.toEqual({ kind: "run", id: "r2", attributes: {} });
  });
});
