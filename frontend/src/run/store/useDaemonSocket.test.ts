import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useDaemonSocket } from "./useDaemonSocket";

/** A socket the test drives: it records every instance and fires its events on demand. */
class FakeSocket {
  static instances: FakeSocket[] = [];
  listeners = new Map<string, ((event: object) => void)[]>();
  url: string;
  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }
  addEventListener(type: string, listener: (event: object) => void) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener]);
  }
  /** Like a browser: the "close" event arrives later, not inside `close()`. */
  close() {}
  /** A "close" carries 1000 (a normal close) unless told otherwise. */
  fire(type: string, event: object = type === "close" ? { code: 1000 } : {}) {
    for (const listener of this.listeners.get(type) ?? []) listener(event);
  }
}

describe("useDaemonSocket", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeSocket.instances = [];
    vi.stubGlobal("WebSocket", FakeSocket);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  it("reconnects after the daemon drops the socket", () => {
    const { result } = renderHook(() => useDaemonSocket());
    expect(FakeSocket.instances).toHaveLength(1);
    act(() => FakeSocket.instances[0].fire("close"));
    expect(result.current.status).toBe("reconnecting");
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    expect(FakeSocket.instances).toHaveLength(2);
  });

  it("asks whether to sign in when the upgrade failed (1006), and only then", () => {
    const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    renderHook(() => useDaemonSocket());
    act(() => FakeSocket.instances[0].fire("close"));
    expect(fetchMock).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(3000);
    });
    act(() => FakeSocket.instances[1].fire("close", { code: 1006 }));
    expect(fetchMock).toHaveBeenCalledWith("/actors/me");
  });

  it("never reconnects once unmounted, even when the close event arrives after", () => {
    const { unmount } = renderHook(() => useDaemonSocket());
    const socket = FakeSocket.instances[0];
    unmount();
    socket.fire("close");
    vi.advanceTimersByTime(10_000);
    expect(FakeSocket.instances).toHaveLength(1);
  });
});
