import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDaemonEvents } from "./useDaemonEvents";
import type { WsMessage } from "../run/model/liveStream";

describe("useDaemonEvents", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  function mount() {
    let listener: ((msg: WsMessage) => void) | null = null;
    const targets = {
      subscribe: (l: (msg: WsMessage) => void) => {
        listener = l;
        return () => {
          listener = null;
        };
      },
      selectedTriggerId: null,
      refreshRuns: vi.fn(),
      refreshRun: vi.fn(),
      refreshSessions: vi.fn(),
      refreshTriggers: vi.fn(),
      refreshProjects: vi.fn(),
      setTriggersPaused: vi.fn(),
      bumpFiresRefreshKey: vi.fn(),
    };
    const hook = renderHook(() => useDaemonEvents(targets));
    return { targets, hook, emit: (msg: WsMessage) => listener?.(msg) };
  }

  it("refreshes once per window for a burst of events", () => {
    const { targets, emit } = mount();
    for (let i = 0; i < 20; i++) emit({ type: "event" });
    expect(targets.refreshRuns).not.toHaveBeenCalled();
    vi.advanceTimersByTime(300);
    expect(targets.refreshRuns).toHaveBeenCalledTimes(1);
    expect(targets.refreshRun).toHaveBeenCalledTimes(1);
    expect(targets.refreshSessions).toHaveBeenCalledTimes(1);
    emit({ type: "event" });
    vi.advanceTimersByTime(300);
    expect(targets.refreshRuns).toHaveBeenCalledTimes(2);
  });

  it("drops a pending refresh once unmounted", () => {
    const { targets, hook, emit } = mount();
    emit({ type: "event" });
    hook.unmount();
    vi.advanceTimersByTime(1_000);
    expect(targets.refreshRuns).not.toHaveBeenCalled();
  });
});
