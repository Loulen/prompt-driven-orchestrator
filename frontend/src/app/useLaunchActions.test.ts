import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { Trigger } from "../shared/generated/Trigger";
import { useLaunchActions } from "./useLaunchActions";

const fireTrigger = vi.fn();
vi.mock("../trigger/api/triggers", () => ({ fireTrigger: (...a: unknown[]) => fireTrigger(...a) }));
vi.mock("../pipeline/store/editStore", () => ({
  useEditStore: (pick: (s: { openPipeline: () => void }) => unknown) => pick({ openPipeline: () => {} }),
}));

const trigger = { id: "trg-1", name: "Nightly", pipeline_id: "p1" } as Trigger;

function setup(triggersPaused: boolean) {
  return renderHook(() =>
    useLaunchActions({
      triggers: [trigger],
      triggersPaused,
      selectRun: vi.fn(),
      setSelectedTriggerId: vi.fn(),
      setSelectedRunId: vi.fn(),
      setSelectedNodeId: vi.fn(),
      setTriggerOpenedTabId: vi.fn(),
    }),
  );
}

describe("useLaunchActions — Run now (#341, #348)", () => {
  beforeEach(() => {
    fireTrigger.mockReset();
    fireTrigger.mockResolvedValue({ fired: true, ok: true });
  });

  it("fires at once while the Triggers run", async () => {
    const { result } = setup(false);
    await act(() => result.current.handleRunNowTrigger(trigger));
    expect(fireTrigger).toHaveBeenCalledWith("trg-1");
    expect(result.current.pausedFire).toBeNull();
  });

  it("asks first while the Triggers are paused, and fires once confirmed", async () => {
    const { result } = setup(true);
    await act(() => result.current.handleRunNowTrigger(trigger));
    expect(fireTrigger).not.toHaveBeenCalled();
    expect(result.current.pausedFire).toBe(trigger);

    act(() => result.current.confirmPausedFire());
    expect(result.current.pausedFire).toBeNull();
    await waitFor(() => expect(fireTrigger).toHaveBeenCalledWith("trg-1"));
  });

  it("fires nothing when the confirmation is cancelled", async () => {
    const { result } = setup(true);
    await act(() => result.current.handleRunNowTrigger(trigger));
    act(() => result.current.cancelPausedFire());
    expect(result.current.pausedFire).toBeNull();
    expect(fireTrigger).not.toHaveBeenCalled();
  });
});
