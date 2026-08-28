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

type Props = { id: string | null; scope?: string; open: boolean };

/** Render the hook with the three arguments it actually takes. */
function render(initial: Props) {
  return renderHook(
    ({ id, scope, open }: Props) => useLibassistLifecycle(id, scope, open),
    { initialProps: initial },
  );
}

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
    render({ id: "alpha", scope: "user", open: true });
    expect(putLibassistFocus).toHaveBeenCalledWith("alpha");
  });

  // The heartbeat is the whole "do not reap while I am editing" mechanism: the
  // daemon's only evidence that a human is still there when no terminal is
  // attached. If it stopped, a user's session would die mid-edit.
  it("keeps re-declaring the focus every 20 s", () => {
    render({ id: "alpha", scope: "user", open: true });
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
    const { rerender } = render({ id: "alpha", scope: "user", open: true });
    expect(putLibassistFocus).toHaveBeenLastCalledWith("alpha");

    rerender({ id: "beta", scope: "library", open: true });
    expect(putLibassistFocus).toHaveBeenLastCalledWith("beta");
    // Switching template must NOT reap: that is the shared session's whole point.
    expect(closeLibraryAssistant).not.toHaveBeenCalled();
  });

  // ADR-0051 §4 reaps on "left EVERY edit view". Glancing at a Run with a
  // template still open is not that — and reaping there costs the conversation,
  // which is the complaint the issue is made of.
  it("survives a round trip to a Run while a template tab stays open", () => {
    const { rerender } = render({ id: "alpha", scope: "user", open: true });
    putLibassistFocus.mockClear();

    rerender({ id: null, scope: undefined, open: true });
    expect(closeLibraryAssistant).not.toHaveBeenCalled();

    // The heartbeat keeps running on the last template edited — stopping it would
    // let the idle TTL reap a session the user is about to come back to.
    act(() => {
      vi.advanceTimersByTime(20_000);
    });
    expect(putLibassistFocus).toHaveBeenLastCalledWith("alpha");

    rerender({ id: "alpha", scope: "user", open: true });
    expect(closeLibraryAssistant).not.toHaveBeenCalled();
  });

  // One call, not two: the daemon clears the focus as part of the reap. Sending a
  // separate `PUT focus: null` is what let the two diverge on `pagehide`, where
  // only one keepalive request gets out — the session died and the focus lingered.
  it("reaps once when the last template tab closes", () => {
    const { rerender } = render({ id: "alpha", scope: "user", open: true });
    putLibassistFocus.mockClear();

    rerender({ id: null, scope: undefined, open: false });

    expect(closeLibraryAssistant).toHaveBeenCalledTimes(1);
    expect(closeLibraryAssistant).toHaveBeenCalledWith();
    expect(putLibassistFocus).not.toHaveBeenCalled();
  });

  it("stops the heartbeat once no edit view is open", () => {
    const { rerender } = render({ id: "alpha", scope: "user", open: true });
    rerender({ id: null, scope: undefined, open: false });
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
    render({ id: "alpha", scope: "user", open: true });

    act(() => {
      window.dispatchEvent(new Event("pagehide"));
    });

    expect(closeLibraryAssistant).toHaveBeenCalledWith({ keepalive: true });
  });

  // The session is shared by every browser tab, so a page that never opened an
  // editor must not reap it: a second tab left on the runs view would otherwise
  // kill the assistant of the tab actually authoring a template.
  it("never reaps from a page that has not been editing", () => {
    render({ id: null, scope: undefined, open: false });
    act(() => {
      window.dispatchEvent(new Event("pagehide"));
    });

    expect(closeLibraryAssistant).not.toHaveBeenCalled();
    expect(putLibassistFocus).not.toHaveBeenCalled();
  });

  it("unregisters the pagehide handler on unmount", () => {
    const { unmount } = render({ id: "alpha", scope: "user", open: true });
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
      const { rerender } = render({ id: "alpha", scope: "user", open: true });
      rerender({ id: null, scope: undefined, open: false });
    }).not.toThrow();
  });
});
