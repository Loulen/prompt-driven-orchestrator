import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { isAtBottom, usePinToBottom } from "./usePinToBottom";

// jsdom lacks ResizeObserver — provide a minimal mock
let resizeCallbacks: (() => void)[] = [];

beforeEach(() => {
  resizeCallbacks = [];
  vi.stubGlobal(
    "ResizeObserver",
    class {
      cb: () => void;
      constructor(cb: () => void) {
        this.cb = cb;
      }
      observe() {
        resizeCallbacks.push(this.cb);
      }
      unobserve() {}
      disconnect() {
        resizeCallbacks = resizeCallbacks.filter((c) => c !== this.cb);
      }
    },
  );
});

function mockPre(scrollHeight: number, scrollTop: number, clientHeight: number) {
  const el = document.createElement("pre");
  Object.defineProperty(el, "scrollHeight", { value: scrollHeight, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: clientHeight, configurable: true });
  Object.defineProperty(el, "scrollTop", { value: scrollTop, writable: true, configurable: true });
  return el;
}

describe("isAtBottom (pure predicate)", () => {
  it("returns true when exactly at bottom", () => {
    expect(isAtBottom({ scrollHeight: 1000, scrollTop: 800, clientHeight: 200 })).toBe(true);
  });

  it("returns true within threshold (< 8px)", () => {
    expect(isAtBottom({ scrollHeight: 1000, scrollTop: 795, clientHeight: 200 })).toBe(true);
  });

  it("returns false when scrolled up beyond threshold", () => {
    expect(isAtBottom({ scrollHeight: 1000, scrollTop: 700, clientHeight: 200 })).toBe(false);
  });

  it("returns true for non-scrollable element (content fits)", () => {
    expect(isAtBottom({ scrollHeight: 200, scrollTop: 0, clientHeight: 200 })).toBe(true);
  });
});

describe("usePinToBottom state machine", () => {
  it("starts pinned to bottom", () => {
    const ref = { current: mockPre(1000, 800, 200) };
    const { result } = renderHook(() => usePinToBottom(ref, "node-a", 1));
    expect(result.current.pinnedToBottom).toBe(true);
  });

  it("unpins when user scrolls up", () => {
    const el = mockPre(1000, 400, 200);
    const ref = { current: el };
    const { result } = renderHook(() => usePinToBottom(ref, "node-a", 1));

    act(() => {
      result.current.handleScroll();
    });

    expect(result.current.pinnedToBottom).toBe(false);
  });

  it("re-pins when user scrolls back to bottom", () => {
    const el = mockPre(1000, 400, 200);
    const ref = { current: el };
    const { result } = renderHook(() => usePinToBottom(ref, "node-a", 1));

    act(() => result.current.handleScroll());
    expect(result.current.pinnedToBottom).toBe(false);

    Object.defineProperty(el, "scrollTop", { value: 795, writable: true, configurable: true });
    act(() => result.current.handleScroll());
    expect(result.current.pinnedToBottom).toBe(true);
  });

  it("scrollToBottom sets scrollTop and re-pins", () => {
    const el = mockPre(1000, 400, 200);
    const ref = { current: el };
    const { result } = renderHook(() => usePinToBottom(ref, "node-a", 1));

    act(() => result.current.handleScroll());
    expect(result.current.pinnedToBottom).toBe(false);

    act(() => result.current.scrollToBottom());
    expect(el.scrollTop).toBe(1000);
    expect(result.current.pinnedToBottom).toBe(true);
  });

  it("resets to pinned when nodeId changes", () => {
    const el = mockPre(1000, 400, 200);
    const ref = { current: el };
    let nodeId = "node-a";
    const { result, rerender } = renderHook(() => usePinToBottom(ref, nodeId, 1));

    act(() => result.current.handleScroll());
    expect(result.current.pinnedToBottom).toBe(false);

    nodeId = "node-b";
    rerender();
    expect(result.current.pinnedToBottom).toBe(true);
  });

  it("resets to pinned when iter changes", () => {
    const el = mockPre(1000, 400, 200);
    const ref = { current: el };
    let iter = 1;
    const { result, rerender } = renderHook(() => usePinToBottom(ref, "node-a", iter));

    act(() => result.current.handleScroll());
    expect(result.current.pinnedToBottom).toBe(false);

    iter = 2;
    rerender();
    expect(result.current.pinnedToBottom).toBe(true);
  });

  it("re-evaluates on resize", () => {
    const el = mockPre(1000, 800, 200);
    const ref = { current: el };
    const { result } = renderHook(() => usePinToBottom(ref, "node-a", 1));

    expect(result.current.pinnedToBottom).toBe(true);

    Object.defineProperty(el, "scrollHeight", { value: 2000, configurable: true });

    act(() => {
      resizeCallbacks.forEach((cb) => cb());
    });

    expect(result.current.pinnedToBottom).toBe(false);
  });

  it("pinnedRef stays in sync with pinnedToBottom", () => {
    const el = mockPre(1000, 400, 200);
    const ref = { current: el };
    const { result } = renderHook(() => usePinToBottom(ref, "node-a", 1));

    expect(result.current.pinnedRef.current).toBe(true);

    act(() => result.current.handleScroll());
    expect(result.current.pinnedRef.current).toBe(false);

    act(() => result.current.scrollToBottom());
    expect(result.current.pinnedRef.current).toBe(true);
  });
});
