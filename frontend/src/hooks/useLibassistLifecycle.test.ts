import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

const { putLibassistFocus, closeLibraryAssistant } = vi.hoisted(() => ({
  putLibassistFocus: vi.fn(),
  closeLibraryAssistant: vi.fn(),
}));
vi.mock("../api", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../api")>();
  return { ...actual, putLibassistFocus, closeLibraryAssistant };
});

import { useLibassistLifecycle } from "./useLibassistLifecycle";

describe("useLibassistLifecycle (#594)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    putLibassistFocus.mockReset().mockResolvedValue({});
    closeLibraryAssistant.mockReset().mockResolvedValue({ ok: true, reaped: true });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("declares the focus as soon as an edit view is open", () => {
    renderHook(() => useLibassistLifecycle("alpha", "user"));
    expect(putLibassistFocus).toHaveBeenCalledWith("alpha", "user");
  });

  // The heartbeat is the whole "do not reap while I am editing" mechanism: the
  // daemon's only evidence that a human is still there when no terminal is
  // attached. If it stopped, a user's session would die mid-edit.
  it("keeps re-declaring the focus every 20 s", () => {
    renderHook(() => useLibassistLifecycle("alpha", "user"));
    expect(putLibassistFocus).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(20_000);
    });
    expect(putLibassistFocus).toHaveBeenCalledTimes(2);

    act(() => {
      vi.advanceTimersByTime(40_000);
    });
    expect(putLibassistFocus).toHaveBeenCalledTimes(4);
  });

  it("re-declares immediately when the edited template changes", () => {
    const { rerender } = renderHook(
      ({ id, scope }: { id: string | null; scope?: string }) =>
        useLibassistLifecycle(id, scope),
      { initialProps: { id: "alpha" as string | null, scope: "user" } },
    );
    expect(putLibassistFocus).toHaveBeenLastCalledWith("alpha", "user");

    rerender({ id: "beta", scope: "library" });
    expect(putLibassistFocus).toHaveBeenLastCalledWith("beta", "library");
    // Switching template must NOT reap: that is the shared session's whole point.
    expect(closeLibraryAssistant).not.toHaveBeenCalled();
  });

  it("clears the focus and reaps when the last edit view closes", () => {
    const { rerender } = renderHook(
      ({ id }: { id: string | null }) => useLibassistLifecycle(id, "user"),
      { initialProps: { id: "alpha" as string | null } },
    );
    putLibassistFocus.mockClear();

    rerender({ id: null });

    expect(putLibassistFocus).toHaveBeenCalledWith(null);
    expect(closeLibraryAssistant).toHaveBeenCalledTimes(1);
  });

  it("stops the heartbeat once no edit view is open", () => {
    const { rerender } = renderHook(
      ({ id }: { id: string | null }) => useLibassistLifecycle(id, "user"),
      { initialProps: { id: "alpha" as string | null } },
    );
    rerender({ id: null });
    putLibassistFocus.mockClear();

    act(() => {
      vi.advanceTimersByTime(120_000);
    });
    expect(putLibassistFocus).not.toHaveBeenCalled();
  });

  // `pagehide`, not `beforeunload`: React runs no effect cleanup on unload, and
  // `beforeunload` does not fire reliably. `keepalive` is what lets the request
  // outlive the document — without it the DELETE is cancelled with the page.
  it("reaps with keepalive on pagehide", () => {
    renderHook(() => useLibassistLifecycle("alpha", "user"));

    act(() => {
      window.dispatchEvent(new Event("pagehide"));
    });

    expect(closeLibraryAssistant).toHaveBeenCalledWith({ keepalive: true });
  });

  // The session is shared by every browser tab, so a page that never opened an
  // editor must not reap it: a second tab left on the runs view would otherwise
  // kill the assistant of the tab actually authoring a template.
  it("never reaps from a page that has not been editing", () => {
    renderHook(() => useLibassistLifecycle(null));
    act(() => {
      window.dispatchEvent(new Event("pagehide"));
    });

    expect(closeLibraryAssistant).not.toHaveBeenCalled();
    expect(putLibassistFocus).not.toHaveBeenCalled();
  });

  it("unregisters the pagehide handler on unmount", () => {
    const { unmount } = renderHook(() => useLibassistLifecycle("alpha", "user"));
    unmount();
    closeLibraryAssistant.mockClear();

    act(() => {
      window.dispatchEvent(new Event("pagehide"));
    });
    expect(closeLibraryAssistant).not.toHaveBeenCalled();
  });

  // Best-effort throughout: a failed declaration must never surface in an editor
  // the user is working in — the next heartbeat re-declares, and the daemon's
  // sweep catches whatever a lost DELETE leaves behind.
  it("swallows API failures", () => {
    putLibassistFocus.mockRejectedValue(new Error("daemon down"));
    closeLibraryAssistant.mockRejectedValue(new Error("daemon down"));

    expect(() => {
      const { rerender } = renderHook(
        ({ id }: { id: string | null }) => useLibassistLifecycle(id, "user"),
        { initialProps: { id: "alpha" as string | null } },
      );
      rerender({ id: null });
    }).not.toThrow();
  });
});
