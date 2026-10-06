import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  HEARTBEAT_TIMEOUT_MS,
  RECONNECT_MAX_MS,
  reconnectDelay,
  useDaemonSocket,
} from "./useDaemonSocket";
import type { WsMessage } from "../types";

// A scriptable socket: the test decides when it opens, speaks or dies.
class FakeWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  static instances: FakeWebSocket[] = [];
  url: string;
  readyState = FakeWebSocket.CONNECTING;
  closeCalls = 0;
  private handlers = new Map<string, Set<(e: unknown) => void>>();
  constructor(url: string) {
    this.url = url;
    FakeWebSocket.instances.push(this);
  }
  addEventListener(type: string, cb: (e: unknown) => void) {
    if (!this.handlers.has(type)) this.handlers.set(type, new Set());
    this.handlers.get(type)!.add(cb);
  }
  removeEventListener(type: string, cb: (e: unknown) => void) {
    this.handlers.get(type)?.delete(cb);
  }
  private fire(type: string, e: unknown = {}) {
    for (const cb of this.handlers.get(type) ?? []) cb(e);
  }
  /** What the hook calls. A half-open socket never answers it with `close`. */
  close() {
    this.closeCalls += 1;
  }
  send() {}
  // --- test drivers ---
  open() {
    this.readyState = FakeWebSocket.OPEN;
    this.fire("open");
  }
  receive(msg: object) {
    this.fire("message", { data: JSON.stringify(msg) });
  }
  serverClose() {
    this.readyState = FakeWebSocket.CLOSED;
    this.fire("close");
  }
}

const sockets = () => FakeWebSocket.instances;
const last = () => sockets()[sockets().length - 1];

function setVisibility(state: "visible" | "hidden") {
  Object.defineProperty(document, "visibilityState", { value: state, configurable: true });
  document.dispatchEvent(new Event("visibilitychange"));
}

function mount() {
  const received: WsMessage[] = [];
  const hook = renderHook(() => useDaemonSocket());
  act(() => {
    hook.result.current.subscribe((msg) => received.push(msg));
  });
  return { hook, received, resyncs: () => received.filter((m) => m.type === "resync").length };
}

describe("useDaemonSocket (#972)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeWebSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeWebSocket);
    Object.defineProperty(document, "visibilityState", { value: "visible", configurable: true });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("backs off exponentially, capped", () => {
    expect([0, 1, 2, 3, 4].map(reconnectDelay)).toEqual([1000, 2000, 4000, 8000, 16000]);
    expect(reconnectDelay(5)).toBe(RECONNECT_MAX_MS);
    expect(reconnectDelay(50)).toBe(RECONNECT_MAX_MS);
  });

  it("spaces its attempts while the daemon is away, and resets once connected", () => {
    const { hook } = mount();
    expect(sockets()).toHaveLength(1);
    act(() => last().serverClose());
    expect(hook.result.current.status).toBe("reconnecting");

    // 1st retry after 1 s, 2nd 2 s after that, 3rd 4 s after that.
    act(() => vi.advanceTimersByTime(999));
    expect(sockets()).toHaveLength(1);
    act(() => vi.advanceTimersByTime(1));
    expect(sockets()).toHaveLength(2);
    act(() => last().serverClose());
    act(() => vi.advanceTimersByTime(1999));
    expect(sockets()).toHaveLength(2);
    act(() => vi.advanceTimersByTime(1));
    expect(sockets()).toHaveLength(3);
    act(() => last().serverClose());
    act(() => vi.advanceTimersByTime(3999));
    expect(sockets()).toHaveLength(3);
    act(() => vi.advanceTimersByTime(1));
    expect(sockets()).toHaveLength(4);

    act(() => last().open());
    expect(hook.result.current.status).toBe("connected");
    // Connected again: the next drop starts the backoff over at 1 s.
    act(() => last().serverClose());
    act(() => vi.advanceTimersByTime(1000));
    expect(sockets()).toHaveLength(5);
  });

  it("asks for a full re-read after a reconnection, never on the first open", () => {
    const { resyncs } = mount();
    act(() => last().open());
    expect(resyncs()).toBe(0);

    act(() => last().serverClose());
    act(() => vi.advanceTimersByTime(1000));
    act(() => last().open());
    expect(resyncs()).toBe(1);
  });

  it("re-reads once the daemon is reachable after a failed first attempt", () => {
    const { resyncs } = mount();
    act(() => last().serverClose());
    act(() => vi.advanceTimersByTime(1000));
    act(() => last().open());
    expect(resyncs()).toBe(1);
  });

  it("closes and reopens a socket that stopped beating", () => {
    const { hook, resyncs } = mount();
    act(() => last().open());
    const first = last();

    // Heartbeats keep it alive.
    act(() => vi.advanceTimersByTime(HEARTBEAT_TIMEOUT_MS - 1));
    act(() => first.receive({ type: "heartbeat", ts: "1" }));
    act(() => vi.advanceTimersByTime(HEARTBEAT_TIMEOUT_MS - 1));
    expect(sockets()).toHaveLength(1);

    // Then silence: the watchdog gives up on it, without waiting for a `close`
    // a half-open socket would never deliver.
    act(() => vi.advanceTimersByTime(1));
    expect(first.closeCalls).toBe(1);
    expect(hook.result.current.status).toBe("reconnecting");
    act(() => vi.advanceTimersByTime(1000));
    expect(sockets()).toHaveLength(2);
    act(() => last().open());
    expect(resyncs()).toBe(1);

    // The dead socket's late `close` schedules nothing more.
    act(() => first.serverClose());
    act(() => vi.advanceTimersByTime(HEARTBEAT_TIMEOUT_MS - 1));
    expect(sockets()).toHaveLength(2);
  });

  it("forwards the daemon's resync to the listeners", () => {
    const { received } = mount();
    act(() => last().open());
    act(() => last().receive({ type: "resync" }));
    act(() => last().receive({ type: "heartbeat" }));
    act(() => last().receive({ type: "event", event: { kind: "node_started" } }));
    expect(received.map((m) => m.type)).toEqual(["resync", "event"]);
  });

  it("re-reads when the tab becomes visible again", () => {
    const { resyncs } = mount();
    act(() => last().open());
    act(() => setVisibility("hidden"));
    expect(resyncs()).toBe(0);
    act(() => {
      vi.advanceTimersByTime(5000);
      last().receive({ type: "heartbeat" });
    });
    act(() => setVisibility("visible"));
    expect(resyncs()).toBe(1);
    expect(sockets()).toHaveLength(1);
  });

  it("reopens at once a socket found silent on return to the tab", () => {
    const { resyncs } = mount();
    act(() => last().open());
    act(() => setVisibility("hidden"));
    // A background tab's timers are throttled: simulate the watchdog not having
    // run by moving the clock without firing timers.
    vi.setSystemTime(Date.now() + 10 * 60_000);
    act(() => setVisibility("visible"));
    expect(sockets()[0].closeCalls).toBe(1);
    act(() => vi.advanceTimersByTime(1000));
    expect(sockets()).toHaveLength(2);
    act(() => last().open());
    expect(resyncs()).toBe(1);
  });

  it("brings a pending retry forward when the tab becomes visible", () => {
    mount();
    act(() => last().serverClose());
    act(() => vi.advanceTimersByTime(1000));
    act(() => last().serverClose());
    // Next retry is 2 s away; coming back to the tab does not wait for it.
    act(() => setVisibility("visible"));
    expect(sockets()).toHaveLength(3);
  });

  it("stops everything on unmount", () => {
    const { hook } = mount();
    act(() => last().open());
    hook.unmount();
    expect(last().closeCalls).toBe(1);
    act(() => vi.advanceTimersByTime(120_000));
    expect(sockets()).toHaveLength(1);
  });
});
