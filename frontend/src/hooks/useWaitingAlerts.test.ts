import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { RunListEntry, RunState, WsMessage } from "../types";
import { installFakeNotification, uninstallFakeNotification } from "../test/fakeNotification";
import { disposeWaitingAlertSettings, initWaitingAlertSettings } from "./useWaitingAlertSettings";
import { WAITING_ALERT_PREFS_STORAGE_KEY } from "../lib/waitingAlertPrefs";

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
  uninstallFakeNotification();
  Reflect.deleteProperty(document, "visibilityState");
  localStorage.clear();
});

function enable() {
  localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify({ notification: true, sound: false }));
  act(() => void initWaitingAlertSettings());
}

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
    await new Promise((r) => setTimeout(r, 10));
    expect(fake.shown).toHaveLength(0);
  });

  it("nothing while the switch is off (the default)", async () => {
    const fake = installFakeNotification("granted");
    act(() => void initWaitingAlertSettings());
    const ws = bus();
    renderHook(() => useWaitingAlerts({ subscribe: ws.subscribe, runs: RUNS, openRun: null, onOpen: vi.fn() }));
    act(() => ws.emit(awaiting({ cause: "declared", message: "x" })));
    await new Promise((r) => setTimeout(r, 10));
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
});
