import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { RunListEntry, RunState, WsMessage } from "../types";
import { installFakeNotification, uninstallFakeNotification } from "../test/fakeNotification";
import { disposeWaitingAlertSettings, initWaitingAlertSettings } from "./useWaitingAlertSettings";
import { WAITING_ALERT_PREFS_STORAGE_KEY } from "../lib/waitingAlertPrefs";
import { installFakeAudioContext, uninstallFakeAudioContext } from "../test/fakeAudioContext";
import { resetSoundAdapter } from "../lib/soundAdapter";
import {
  broadcastChannelTransport,
  createTabCoordinator,
  disposeTabCoordinator,
  initTabCoordinator,
  type TabCoordinator,
} from "../lib/tabCoordinator";

const fetchRunMock = vi.fn();
vi.mock("../api", () => ({ fetchRun: (...args: unknown[]) => fetchRunMock(...args) }));

import { useWaitingAlerts } from "./useWaitingAlerts";

let visibility: DocumentVisibilityState = "hidden";

function bus() {
  const listeners = new Set<(msg: WsMessage) => void>();
  return {
    subscribe: (l: (msg: WsMessage) => void) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
    emit: (msg: WsMessage) => listeners.forEach((l) => l(msg)),
  };
}

function awaiting(payload: Record<string, unknown>, run = "run-1", node = "ask"): WsMessage {
  return {
    type: "event",
    event: { id: 1, run_id: run, ts: "t", kind: "node_awaiting_user", node_id: node, iter: 1, payload },
  };
}

const RUNS = [{ run_id: "run-1", name: "feature-login", pipeline_name: "p", status: "running" }] as RunListEntry[];

beforeEach(() => {
  localStorage.clear();
  visibility = "hidden";
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
  fetchRunMock.mockReset();
});

afterEach(() => {
  disposeWaitingAlertSettings();
  disposeTabCoordinator();
  uninstallFakeNotification();
  uninstallFakeAudioContext();
  resetSoundAdapter();
  Reflect.deleteProperty(document, "visibilityState");
  localStorage.clear();
});

function enable(prefs = { notification: true, sound: false }) {
  localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify(prefs));
  act(() => void initWaitingAlertSettings());
}

/** Let the claim window close (and a BroadcastChannel deliver). */
const settle = (ms = 400) => act(() => new Promise((r) => setTimeout(r, ms)));

describe("useWaitingAlerts (#1011)", () => {
  it("shows the notification of a live wait while the tab is hidden; its click opens the Run + node", async () => {
    const fake = installFakeNotification("granted");
    enable();
    const onOpen = vi.fn();
    const ws = bus();
    renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen }));
    act(() => ws.emit(awaiting({ cause: "declared", message: "Which layout?" })));
    await waitFor(() => expect(fake.shown).toHaveLength(1));
    expect(fake.shown[0]).toMatchObject({
      title: "PDO — feature-login is waiting for you",
      body: "Which layout?",
      tag: "run-1:ask:1",
    });
    expect(fetchRunMock).not.toHaveBeenCalled();
    const focus = vi.spyOn(window, "focus").mockImplementation(() => {});
    act(() => fake.shown[0].onclick?.());
    expect(focus).toHaveBeenCalled();
    expect(onOpen).toHaveBeenCalledWith({ runId: "run-1", nodeId: "ask" });
    focus.mockRestore();
  });

  it("nothing while the tab is in view, or on resync", async () => {
    const fake = installFakeNotification("granted");
    enable();
    const ws = bus();
    renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
    visibility = "visible";
    act(() => ws.emit(awaiting({ cause: "declared", message: "x" })));
    visibility = "hidden";
    act(() => ws.emit({ type: "resync" }));
    await settle();
    expect(fake.shown).toHaveLength(0);
  });

  it("nothing while the switch is off (the default)", async () => {
    const fake = installFakeNotification("granted");
    act(() => void initWaitingAlertSettings());
    const ws = bus();
    renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
    act(() => ws.emit(awaiting({ cause: "declared", message: "x" })));
    await settle();
    expect(fake.shown).toHaveLength(0);
  });

  it("fetches the Run when it is not listed yet, for its name and the node's", async () => {
    const fake = installFakeNotification("granted");
    enable();
    fetchRunMock.mockResolvedValue({
      run_id: "run-2",
      name: null,
      pipeline_name: "grill",
      node_defs: [{ id: "ask", name: "Griller", node_type: "agent" }],
    } as unknown as RunState);
    const ws = bus();
    renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: [], openRun: null, onOpen: vi.fn() }));
    act(() => ws.emit(awaiting({ cause: "completion_not_released" }, "run-2")));
    await waitFor(() => expect(fake.shown).toHaveLength(1));
    expect(fake.shown[0].title).toBe("PDO — grill is waiting for you");
    expect(fake.shown[0].body).toBe("Node Griller is done and waits for your go");
  });

  it("falls back on the ids when the Run cannot be read", async () => {
    const fake = installFakeNotification("granted");
    enable();
    fetchRunMock.mockRejectedValue(new Error("404"));
    const ws = bus();
    renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: [], openRun: null, onOpen: vi.fn() }));
    act(() => ws.emit(awaiting({ cause: "declared" }, "run-9", "n1")));
    await waitFor(() => expect(fake.shown).toHaveLength(1));
    expect(fake.shown[0].title).toBe("PDO — run-9 is waiting for you");
    expect(fake.shown[0].body).toBe("Node n1 is waiting for you");
  });

  describe("sound and tabs (#1012)", () => {
    function event(id: number, payload: Record<string, unknown>, node = "ask"): WsMessage {
      return {
        type: "event",
        event: { id, run_id: "run-1", ts: `t${id}`, kind: "node_awaiting_user", node_id: node, iter: 1, payload },
      };
    }

    it("plays the chime of the reason, with the notification", async () => {
      const fake = installFakeNotification("granted");
      const audio = installFakeAudioContext();
      enable({ notification: true, sound: true });
      const ws = bus();
      renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
      act(() => ws.emit(event(1, { cause: "declared", message: "Which layout?" })));
      await waitFor(() => expect(fake.shown).toHaveLength(1));
      await waitFor(() => expect(audio.chimes).toHaveLength(1));
      // Celesta: C6 then G5.
      expect(audio.chimes[0].frequencies).toEqual([1046.5, 783.99]);
    });

    it("sound alone, notification blocked: the chime still plays", async () => {
      installFakeNotification("denied");
      const audio = installFakeAudioContext();
      enable({ notification: false, sound: true });
      const ws = bus();
      renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
      act(() => ws.emit(event(1, { cause: "completion_not_released" })));
      await waitFor(() => expect(audio.chimes).toHaveLength(1));
      expect(audio.chimes[0].frequencies).toContain(523.25); // Kalimba triad's C5
    });

    it("a burst: one notification per transition, a single chime in 10 s", async () => {
      const fake = installFakeNotification("granted");
      const audio = installFakeAudioContext();
      enable({ notification: true, sound: true });
      const ws = bus();
      renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
      act(() => ws.emit(event(1, { cause: "declared", message: "first" })));
      await waitFor(() => expect(fake.shown).toHaveLength(1));
      act(() => ws.emit(event(2, { cause: "declared", message: "second" }, "other")));
      await waitFor(() => expect(fake.shown).toHaveLength(2));
      act(() => ws.emit(event(3, { cause: "completion_not_released" }, "third")));
      await waitFor(() => expect(fake.shown).toHaveLength(3));
      await settle();
      expect(audio.chimes).toHaveLength(1);
    });

    it("another PDO tab of this browser in view: no tab alerts", async () => {
      const fake = installFakeNotification("granted");
      const audio = installFakeAudioContext();
      enable({ notification: true, sound: true });
      act(() => void initTabCoordinator({ canPlaySound: () => true }));
      // A second tab, visible, on the real BroadcastChannel.
      const peer: TabCoordinator = createTabCoordinator({
        transport: broadcastChannelTransport()!,
        tabId: "peer",
        isVisible: () => true,
        canPlaySound: () => true,
      });
      try {
        await settle(50);
        const ws = bus();
        renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
        act(() => ws.emit(event(1, { cause: "declared", message: "x" })));
        await settle();
        expect(fake.shown).toHaveLength(0);
        expect(audio.chimes).toHaveLength(0);
      } finally {
        peer.dispose();
      }
    });

    it("a transition another tab already claimed is not emitted here", async () => {
      const fake = installFakeNotification("granted");
      const audio = installFakeAudioContext();
      enable({ notification: true, sound: true });
      act(() => void initTabCoordinator({ canPlaySound: () => true }));
      const peer = createTabCoordinator({
        transport: broadcastChannelTransport()!,
        tabId: "peer",
        isVisible: () => false,
        canPlaySound: () => true,
      });
      try {
        const msg = event(7, { cause: "declared", message: "x" });
        const peerWon = peer.claim("run-1:ask:1#7#node_awaiting_user");
        await settle(50);
        const ws = bus();
        renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
        act(() => ws.emit(msg));
        await settle();
        await expect(peerWon).resolves.toBe(true);
        expect(fake.shown).toHaveLength(0);
        expect(audio.chimes).toHaveLength(0);
      } finally {
        peer.dispose();
      }
    });
  });
});
