/**
 * The canvas framing a graph beside the right pane: a tab shown for the first
 * time is fitted, and a fit nobody moved is fitted again when the pane unfolds,
 * so every card of a fresh Run stays in frame (End included).
 */
import { renderHook, act } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactFlowInstance } from "@xyflow/react";
import { MOUNT_FIT_WINDOW_MS } from "../../lib/paneResizeViewport";
import { usePaneToggleReframe } from "./usePaneToggleReframe";

let observed: (() => void) | null = null;
class FakeResizeObserver {
  constructor(cb: () => void) {
    observed = cb;
  }
  observe() {}
  disconnect() {}
}

let width = 1300;
const wrapper = {
  getBoundingClientRect: () => ({ width, height: 800 }) as DOMRect,
} as unknown as HTMLDivElement;
const ref = { current: wrapper };

let viewport = { x: 0, y: 0, zoom: 1 };
const reactFlow = {
  fitView: vi.fn(async () => {
    viewport = { x: 10, y: 20, zoom: 0.8 };
    return true;
  }),
  getViewport: () => viewport,
  setViewport: vi.fn((v: typeof viewport) => {
    viewport = v;
  }),
  getNodes: () => [],
  getNodesBounds: () => ({ x: 0, y: 0, width: 0, height: 0 }),
  getInternalNode: () => undefined,
} as unknown as ReactFlowInstance;

type Props = { tabId: string | undefined; collapsed: boolean };
function mount(initial: Props) {
  return renderHook(
    ({ tabId, collapsed }: Props) => usePaneToggleReframe(ref, reactFlow, tabId != null, collapsed, tabId),
    { initialProps: initial },
  );
}

/** The right pane unfolds: the canvas narrows, and the observer measures it. */
async function unfold(rerender: (p: Props) => void, tabId: string) {
  rerender({ tabId, collapsed: false });
  width = 900;
  await act(async () => {
    observed?.();
  });
}

describe("usePaneToggleReframe", () => {
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["performance", "setTimeout", "clearTimeout"] });
    vi.stubGlobal("ResizeObserver", FakeResizeObserver);
    width = 1300;
    viewport = { x: 0, y: 0, zoom: 1 };
    vi.mocked(reactFlow.fitView).mockClear();
    vi.mocked(reactFlow.setViewport).mockClear();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("fits a tab shown for the first time, and again when the pane unfolds right after", async () => {
    const { rerender } = mount({ tabId: "template", collapsed: true });
    await act(async () => {
      observed?.();
    });
    act(() => {
      vi.advanceTimersByTime(MOUNT_FIT_WINDOW_MS * 5);
    });
    vi.mocked(reactFlow.fitView).mockClear();

    // A fresh Run opens in a new tab of the same canvas.
    rerender({ tabId: "run-1", collapsed: true });
    expect(reactFlow.fitView).toHaveBeenCalledTimes(1);
    await unfold(rerender, "run-1");
    expect(reactFlow.fitView).toHaveBeenCalledTimes(2);
    expect(reactFlow.setViewport).not.toHaveBeenCalled();
  });

  it("does not fit a tab already framed when the reader comes back to it", () => {
    const { rerender } = mount({ tabId: "a", collapsed: true });
    rerender({ tabId: "b", collapsed: true });
    vi.mocked(reactFlow.fitView).mockClear();
    rerender({ tabId: "a", collapsed: true });
    expect(reactFlow.fitView).not.toHaveBeenCalled();
  });

  it("fits again on a late unfold while the fitted view was not moved", async () => {
    const { rerender } = mount({ tabId: "run-1", collapsed: true });
    await act(async () => {
      observed?.();
    });
    act(() => {
      vi.advanceTimersByTime(MOUNT_FIT_WINDOW_MS * 5);
    });
    vi.mocked(reactFlow.fitView).mockClear();
    await unfold(rerender, "run-1");
    expect(reactFlow.fitView).toHaveBeenCalledTimes(1);
    expect(reactFlow.setViewport).not.toHaveBeenCalled();
  });

  it("re-frames, keeping the zoom, a view the reader moved", async () => {
    const { rerender } = mount({ tabId: "run-1", collapsed: true });
    await act(async () => {
      observed?.();
    });
    act(() => {
      vi.advanceTimersByTime(MOUNT_FIT_WINDOW_MS * 5);
    });
    viewport = { x: -300, y: 40, zoom: 1.5 };
    vi.mocked(reactFlow.fitView).mockClear();
    await unfold(rerender, "run-1");
    expect(reactFlow.fitView).not.toHaveBeenCalled();
    expect(reactFlow.setViewport).toHaveBeenCalledTimes(1);
  });
});
