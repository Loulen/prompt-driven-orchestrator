import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDaemonEvents } from "./useDaemonEvents";
import { useEditStore } from "../pipeline/store/editStore";
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
      refreshTriggersPaused: vi.fn(),
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

  it("reads every Trigger again on a trigger_updated, with or without its id", () => {
    const { targets, emit } = mount();
    emit({ type: "trigger_updated", trigger_id: "t1" });
    emit({ type: "trigger_updated" });
    expect(targets.refreshTriggers).toHaveBeenCalledTimes(2);
    expect(targets.refreshRuns).not.toHaveBeenCalled();
  });

  it("reads the Projets again on a project_changed", () => {
    const { targets, emit } = mount();
    emit({ type: "project_changed", project_id: "p1" });
    expect(targets.refreshProjects).toHaveBeenCalledTimes(1);
    expect(targets.refreshTriggers).not.toHaveBeenCalled();
  });

  it("drops a pending refresh once unmounted", () => {
    const { targets, hook, emit } = mount();
    emit({ type: "event" });
    hook.unmount();
    vi.advanceTimersByTime(1_000);
    expect(targets.refreshRuns).not.toHaveBeenCalled();
  });

  it("reads every list again on a resync", () => {
    const original = useEditStore.getState().loadPipelines;
    const loadPipelines = vi.fn(async () => {});
    useEditStore.setState({ loadPipelines });
    const { targets, emit } = mount();
    // A burst of them costs one read of each, in the window.
    emit({ type: "resync" });
    emit({ type: "resync" });
    expect(targets.refreshTriggers).not.toHaveBeenCalled();
    vi.advanceTimersByTime(300);
    expect(targets.refreshTriggers).toHaveBeenCalledTimes(1);
    expect(targets.refreshTriggersPaused).toHaveBeenCalledTimes(1);
    expect(targets.refreshProjects).toHaveBeenCalledTimes(1);
    expect(loadPipelines).toHaveBeenCalledTimes(1);
    expect(targets.refreshRuns).toHaveBeenCalledTimes(1);
    expect(targets.refreshRun).toHaveBeenCalledTimes(1);
    expect(targets.refreshSessions).toHaveBeenCalledTimes(1);
    expect(targets.bumpFiresRefreshKey).not.toHaveBeenCalled();
    useEditStore.setState({ loadPipelines: original });
  });
});
