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

  it("tells the open launch forms on a project_changed and a pipeline_validation_changed", async () => {
    const { LAUNCH_OFFER_CHANGED } = await import("../run/store/useLaunchTargets");
    const heard = vi.fn();
    window.addEventListener(LAUNCH_OFFER_CHANGED, heard);
    try {
      const { emit } = mount();
      emit({ type: "project_changed", project_id: "p1" });
      emit({ type: "pipeline_validation_changed", pipeline_id: "shared" });
      expect(heard).toHaveBeenCalledTimes(2);
    } finally {
      window.removeEventListener(LAUNCH_OFFER_CHANGED, heard);
    }
  });

  it("tells the open launch forms when a Pipeline file is written or numbered, and on a resync", async () => {
    const { LAUNCH_OFFER_CHANGED } = await import("../run/store/useLaunchTargets");
    const original = useEditStore.getState();
    useEditStore.setState({ loadPipelines: vi.fn(async () => {}), reloadPipeline: vi.fn(async () => {}) });
    const heard = vi.fn();
    window.addEventListener(LAUNCH_OFFER_CHANGED, heard);
    try {
      const { emit } = mount();
      emit({ type: "pipeline_changed", pipeline_id: "nightly" });
      emit({ type: "pipeline_changed" });
      emit({ type: "pipeline_version_changed", pipeline_id: "nightly" });
      expect(heard).toHaveBeenCalledTimes(3);
      emit({ type: "resync" });
      vi.advanceTimersByTime(1_000);
      expect(heard).toHaveBeenCalledTimes(4);
    } finally {
      window.removeEventListener(LAUNCH_OFFER_CHANGED, heard);
      useEditStore.setState({ loadPipelines: original.loadPipelines, reloadPipeline: original.reloadPipeline });
    }
  });

  it("tells the model pickers on a harness_catalogue_changed", async () => {
    const { HARNESS_CATALOGUE_CHANGED } = await import("../harness/store/useHarnessCatalog");
    const heard = vi.fn();
    window.addEventListener(HARNESS_CATALOGUE_CHANGED, heard);
    try {
      const { targets, emit } = mount();
      emit({ type: "harness_catalogue_changed", harness: "claude" });
      expect(heard).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(1_000);
      expect(targets.refreshRuns).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener(HARNESS_CATALOGUE_CHANGED, heard);
    }
  });

  it("tells an open Settings page and New Run on an instance_settings_changed (CORE-232)", async () => {
    const { SETTINGS_WRITTEN, SETTINGS_CHANGED } = await import("../settings/store/useSettings");
    const written = vi.fn();
    const changed = vi.fn();
    window.addEventListener(SETTINGS_WRITTEN, written);
    window.addEventListener(SETTINGS_CHANGED, changed);
    try {
      const { targets, emit } = mount();
      emit({ type: "instance_settings_changed" });
      expect(written).toHaveBeenCalledTimes(1);
      expect(changed).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(1_000);
      expect(targets.refreshRuns).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener(SETTINGS_WRITTEN, written);
      window.removeEventListener(SETTINGS_CHANGED, changed);
    }
  });

  it("reads the Pipelines again on a pipeline_validation_changed", async () => {
    const { useEditStore } = await import("../pipeline/store/editStore");
    const original = useEditStore.getState().loadPipelines;
    const loadPipelines = vi.fn(async () => {});
    useEditStore.setState({ loadPipelines });
    try {
      const { targets, emit } = mount();
      emit({ type: "pipeline_validation_changed", pipeline_id: "shared" });
      expect(loadPipelines).toHaveBeenCalledTimes(1);
      vi.advanceTimersByTime(1_000);
      expect(targets.refreshRuns).not.toHaveBeenCalled();
    } finally {
      useEditStore.setState({ loadPipelines: original });
    }
  });

  it("reads the Pipelines again on a pipeline_version_changed, and reloads no editor (ADR-0088)", () => {
    const original = useEditStore.getState();
    const loadPipelines = vi.fn(async () => {});
    const reloadPipeline = vi.fn(async () => {});
    useEditStore.setState({ loadPipelines, reloadPipeline });
    try {
      const { emit } = mount();
      emit({ type: "pipeline_version_changed", pipeline_id: "shared" });
      expect(loadPipelines).toHaveBeenCalledTimes(1);
      expect(reloadPipeline).not.toHaveBeenCalled();
    } finally {
      useEditStore.setState({ loadPipelines: original.loadPipelines, reloadPipeline: original.reloadPipeline });
    }
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
