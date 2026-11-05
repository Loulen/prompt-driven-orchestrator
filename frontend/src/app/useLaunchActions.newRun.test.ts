/** API 0.22: the app shell's New Run is the one an extension's UI opens. */
import { describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";
import { useLaunchActions } from "./useLaunchActions";
import { openNewRun } from "../shared/extensions/newRun";
import { RUN_INTENT } from "../run/components/NewRunModal";

function mount() {
  return renderHook(() =>
    useLaunchActions({
      triggers: [],
      triggersPaused: false,
      selectRun: vi.fn(),
      setSelectedTriggerId: vi.fn(),
      setSelectedRunId: vi.fn(),
      setSelectedNodeId: vi.fn(),
      setTriggerOpenedTabId: vi.fn(),
    }),
  );
}

describe("useLaunchActions — New Run opened by an extension", () => {
  it("opens the modal on the prefill it is handed", () => {
    const { result, unmount } = mount();
    act(() => {
      openNewRun({ project: "p1", name: "n", extension: "test", data: { cle: 1 } });
    });
    expect(result.current.newRunModalOpen).toBe(true);
    // Never over the one already open: its draft stays whole.
    let again = true;
    act(() => {
      again = openNewRun({ name: "other" });
    });
    expect(again).toBe(false);
    expect(result.current.openIntent).toEqual({
      kind: "run",
      prefill: { project: "p1", name: "n", extension: "test", data: { cle: 1 } },
    });
    // Closed, it opens again.
    act(() => result.current.handleCloseNewRunModal());
    let reopened = false;
    act(() => {
      reopened = openNewRun({ name: "other" });
    });
    expect(reopened).toBe(true);
    unmount();
    expect(openNewRun()).toBe(false);
  });

  it("opens a plain New Run without a prefill", () => {
    const { result, unmount } = mount();
    act(() => {
      openNewRun();
    });
    expect(result.current.newRunModalOpen).toBe(true);
    expect(result.current.openIntent).toBe(RUN_INTENT);
    unmount();
  });
});
