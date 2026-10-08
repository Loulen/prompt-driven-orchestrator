import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { STREAM_OPEN_MARK, STREAM_SILENCE_MS, useDaemonSocket } from "./useDaemonSocket";
import { useReviewLive } from "../../review/store/useReviewLive";
import type { WsMessage } from "../model/liveStream";

/** A socket the test drives: it records every instance and fires its events on demand. */
class FakeSocket {
  static OPEN = 1;
  static instances: FakeSocket[] = [];
  listeners = new Map<string, ((event: object) => void)[]>();
  url: string;
  readyState = 0;
  binaryType = "blob";
  closedWith: number | undefined;
  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }
  addEventListener(type: string, listener: (event: object) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  /** Like a browser: the "close" event arrives later, not inside `close()`. */
  close(code?: number) {
    this.closedWith = code;
  }
  fire(type: string, event: object = {}) {
    if (type === "open") this.readyState = 1;
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

/** Let the probe answer and the socket open. */
async function settle() {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(0);
  });
}

describe("useDaemonSocket", () => {
  let fetchMock: ReturnType<typeof vi.fn>;
  beforeEach(() => {
    vi.useFakeTimers();
    FakeSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeSocket);
    fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("opens /ws once the daemon answered the probe, and marks it open", async () => {
    const mark = vi.spyOn(performance, "mark");
    const { result, unmount } = renderHook(() => useDaemonSocket());
    expect(FakeSocket.instances).toHaveLength(0);
    expect(result.current.status).toBe("disconnected");
    await settle();
    expect(fetchMock).toHaveBeenCalledWith("/actors/me", expect.anything());
    expect(FakeSocket.instances).toHaveLength(1);
    expect(FakeSocket.instances[0].url).toMatch(/\/ws$/);
    act(() => FakeSocket.instances[0].fire("open"));
    expect(result.current.status).toBe("connected");
    expect(mark).toHaveBeenCalledWith(STREAM_OPEN_MARK);
    unmount();
  });

  it("relays the stream's messages to every subscriber, nothing else", async () => {
    const { result, unmount } = renderHook(() => useDaemonSocket());
    await settle();
    const seen: WsMessage[] = [];
    const stop = result.current.subscribe((msg) => seen.push(msg));
    const ws = FakeSocket.instances[0];
    act(() => {
      ws.fire("open");
      ws.fire("message", { data: JSON.stringify({ type: "ready" }) });
      ws.fire("message", { data: JSON.stringify({ type: "event", event: { run_id: "r1" } }) });
      ws.fire("message", { data: "not json" });
    });
    // The opening re-reads, then the Run's event.
    expect(seen.map((m) => m.type)).toEqual(["resync", "event"]);
    stop();
    unmount();
  });

  it("relays the changes of the Triggers, the Projets and the validations made elsewhere", async () => {
    const { result, unmount } = renderHook(() => useDaemonSocket());
    await settle();
    const seen: WsMessage[] = [];
    const stop = result.current.subscribe((msg) => seen.push(msg));
    const ws = FakeSocket.instances[0];
    act(() => ws.fire("open"));
    seen.length = 0;
    act(() => {
      ws.fire("message", { data: JSON.stringify({ type: "trigger_updated", trigger_id: "t1" }) });
      // An extension's signal: every Trigger, no id.
      ws.fire("message", { data: JSON.stringify({ type: "trigger_updated" }) });
      ws.fire("message", { data: JSON.stringify({ type: "project_changed", project_id: "p1" }) });
      ws.fire("message", {
        data: JSON.stringify({ type: "pipeline_validation_changed", pipeline_id: "shared" }),
      });
    });
    expect(seen).toEqual([
      { type: "trigger_updated", trigger_id: "t1" },
      { type: "trigger_updated" },
      { type: "project_changed", project_id: "p1" },
      { type: "pipeline_validation_changed", pipeline_id: "shared" },
    ]);
    stop();
    unmount();
  });

  it("probes again, never reopens blindly, after the daemon drops the socket", async () => {
    const { result, unmount } = renderHook(() => useDaemonSocket());
    await settle();
    act(() => FakeSocket.instances[0].fire("open"));
    fetchMock.mockImplementation(async () => {
      throw new TypeError("NetworkError");
    });
    act(() => FakeSocket.instances[0].fire("close", { code: 1006 }));
    expect(result.current.status).toBe("reconnecting");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(10_000);
    });
    expect(FakeSocket.instances).toHaveLength(1);
    unmount();
  });

  it("is one socket for the App and the Review page together", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => new Response("{}", { status: 200 })),
    );
    const app = renderHook(() => useDaemonSocket());
    const review = renderHook(() =>
      useReviewLive({
        runId: "r1",
        runRef: { current: null },
        setRun: () => {},
        sigAtLoad: { current: null },
        pairRef: { current: { from: "fork", to: "tip" } },
        onTipMoved: () => {},
      }),
    );
    await settle();
    expect(FakeSocket.instances).toHaveLength(1);

    // The App leaves (the route changed): the Review page keeps the stream.
    app.unmount();
    expect(FakeSocket.instances[0].closedWith).toBeUndefined();
    review.unmount();
    expect(FakeSocket.instances[0].closedWith).toBe(1000);
  });

  it("never reconnects once its last user left, even when the close event arrives after", async () => {
    const { unmount } = renderHook(() => useDaemonSocket());
    await settle();
    const socket = FakeSocket.instances[0];
    unmount();
    socket.fire("close", { code: 1000 });
    await vi.advanceTimersByTimeAsync(60_000);
    expect(FakeSocket.instances).toHaveLength(1);
  });

  describe("the signal to re-read everything", () => {
    let visibility: DocumentVisibilityState = "visible";
    beforeEach(() => {
      visibility = "visible";
      Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    });
    const setVisibility = (next: DocumentVisibilityState) => {
      visibility = next;
      document.dispatchEvent(new Event("visibilitychange"));
    };

    /** The stream open, with what its subscriber received since. */
    async function openStream() {
      const hook = renderHook(() => useDaemonSocket());
      await settle();
      const seen: string[] = [];
      const stop = hook.result.current.subscribe((msg) => seen.push(msg.type));
      act(() => FakeSocket.instances[0].fire("open"));
      return { hook, seen, stop };
    }

    it("goes out at the opening and at each reopening of the stream", async () => {
      vi.spyOn(Math, "random").mockReturnValue(1);
      const { hook, seen } = await openStream();
      expect(seen).toEqual(["resync"]);
      act(() => FakeSocket.instances[0].fire("close", { code: 1006 }));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      act(() => FakeSocket.instances[1].fire("open"));
      expect(seen).toEqual(["resync", "resync"]);
      hook.unmount();
    });

    it("goes out when the tab shows again, not while it hides", async () => {
      const { hook, seen } = await openStream();
      seen.length = 0;
      act(() => setVisibility("hidden"));
      expect(seen).toEqual([]);
      act(() => setVisibility("visible"));
      expect(seen).toEqual(["resync"]);
      hook.unmount();
    });

    it("goes out when the network is back", async () => {
      const { hook, seen } = await openStream();
      seen.length = 0;
      act(() => {
        window.dispatchEvent(new Event("online"));
      });
      expect(seen).toEqual(["resync"]);
      hook.unmount();
    });

    it("relays the daemon's resync", async () => {
      const { hook, seen } = await openStream();
      seen.length = 0;
      act(() => FakeSocket.instances[0].fire("message", { data: JSON.stringify({ type: "resync" }) }));
      expect(seen).toEqual(["resync"]);
      hook.unmount();
    });

    it("closes a silent stream and re-reads once it is open again", async () => {
      vi.spyOn(Math, "random").mockReturnValue(1);
      const { hook, seen } = await openStream();
      seen.length = 0;
      const ws = FakeSocket.instances[0];
      // The beats keep it open.
      for (let i = 0; i < 3; i++) {
        await act(async () => {
          await vi.advanceTimersByTimeAsync(STREAM_SILENCE_MS - 1_000);
        });
        act(() => ws.fire("message", { data: JSON.stringify({ type: "heartbeat" }) }));
      }
      expect(hook.result.current.status).toBe("connected");
      // Then nothing: dropped, probed, reopened.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(STREAM_SILENCE_MS);
      });
      expect(hook.result.current.status).toBe("reconnecting");
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1_000);
      });
      expect(FakeSocket.instances).toHaveLength(2);
      act(() => FakeSocket.instances[1].fire("open"));
      expect(seen).toEqual(["resync"]);
      hook.unmount();
    });

    it("stops listening to the tab and the network once its last user left", async () => {
      const { hook, seen } = await openStream();
      seen.length = 0;
      hook.unmount();
      act(() => {
        setVisibility("visible");
        window.dispatchEvent(new Event("online"));
      });
      expect(seen).toEqual([]);
    });
  });
});
