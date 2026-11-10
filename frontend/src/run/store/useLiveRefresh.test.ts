import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SAFETY_NET_MS, useLiveRefresh } from "./useLiveRefresh";
import { useDaemonSocket } from "./useDaemonSocket";
import { concernsChildren } from "../model/orchestration";
import type { WsMessage } from "../model/liveStream";

class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  listeners = new Map<string, ((event: object) => void)[]>();
  readyState = 0;
  binaryType = "blob";
  url: string;
  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }
  addEventListener(type: string, listener: (event: object) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  close() {}
  fire(type: string, event: object = {}) {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

const eventOf = (run_id: string, kind = "node_started"): WsMessage =>
  ({ type: "event", event: { id: 1, run_id, ts: "", kind, node_id: null, iter: null, payload: null } }) as WsMessage;

describe("useLiveRefresh", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeSocket);
    vi.stubGlobal("fetch", vi.fn(async () => new Response("{}", { status: 200 })));
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  const matches = (msg: WsMessage) => msg.type === "event" && msg.event?.run_id === "r1";

  it("polls while the event stream is not open", () => {
    const load = vi.fn();
    renderHook(() => useLiveRefresh(load, matches, 1000, true));
    vi.advanceTimersByTime(3000);
    expect(load).toHaveBeenCalledTimes(3);
  });

  it("does not poll while the stream is open: one load per window of matching events, and the safety net", async () => {
    const stream = renderHook(() => useDaemonSocket());
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    act(() => FakeSocket.instances[0].fire("open"));

    const load = vi.fn();
    renderHook(() => useLiveRefresh(load, matches, 1000, true));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5000);
    });
    expect(load).not.toHaveBeenCalled();

    const ws = FakeSocket.instances[0];
    act(() => {
      for (let i = 0; i < 5; i++) ws.fire("message", { data: JSON.stringify(eventOf("r1")) });
      ws.fire("message", { data: JSON.stringify(eventOf("other")) });
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(300);
    });
    expect(load).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(SAFETY_NET_MS);
    });
    expect(load).toHaveBeenCalledTimes(2);
    stream.unmount();
  });
});

describe("concernsChildren", () => {
  const data = {
    run_id: "parent",
    nodes: [{ node_id: "worker", children: [{ run_id: "child", pipeline_name: "p", status: "running" as const }] }],
  };

  it("keeps the parent's events, its known children's and a new child's start", () => {
    expect(concernsChildren(eventOf("parent"), "parent", data)).toBe(true);
    expect(concernsChildren(eventOf("child"), "parent", data)).toBe(true);
    expect(concernsChildren(eventOf("stranger"), "parent", data)).toBe(false);
    expect(concernsChildren(eventOf("grandchild", "node_awaiting_user"), "parent", data)).toBe(true);
    const started = eventOf("new", "run_started");
    started.event!.payload = { parent_run_id: "parent" };
    expect(concernsChildren(started, "parent", data)).toBe(true);
    expect(concernsChildren({ type: "heartbeat" }, "parent", data)).toBe(false);
  });
});
