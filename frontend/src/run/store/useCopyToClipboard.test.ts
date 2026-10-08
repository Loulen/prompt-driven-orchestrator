import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  CLIPBOARD_REFUSED,
  COPIED_MS,
  FAILED_MS,
  LOAD_FAILED,
  useCopyToClipboard,
} from "./useCopyToClipboard";

function setClipboard(clipboard: unknown) {
  Object.defineProperty(navigator, "clipboard", {
    value: clipboard,
    configurable: true,
    writable: true,
  });
}

/** The `execCommand("copy")` fallback of an insecure context (plain http). */
function setExecCommand(execCommand: unknown) {
  Object.defineProperty(document, "execCommand", {
    value: execCommand,
    configurable: true,
    writable: true,
  });
}

describe("useCopyToClipboard (#41)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
    setClipboard(undefined);
    setExecCommand(undefined);
  });

  it("goes copied once writeText resolves, then back to idle after 1.5 s", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    const { result } = renderHook(() => useCopyToClipboard());

    await act(() => result.current.copy("---\nk: v\n---\nbody"));
    expect(writeText).toHaveBeenCalledWith("---\nk: v\n---\nbody");
    expect(result.current.status).toBe("copied");

    act(() => vi.advanceTimersByTime(COPIED_MS - 1));
    expect(result.current.status).toBe("copied");
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.status).toBe("idle");
  });

  it("never reports copied before writeText resolves", async () => {
    let resolve!: () => void;
    const writeText = vi.fn(() => new Promise<void>((r) => (resolve = r)));
    setClipboard({ writeText });
    const { result } = renderHook(() => useCopyToClipboard());

    let pending!: Promise<void>;
    act(() => {
      pending = result.current.copy("text");
    });
    await act(async () => {});
    expect(result.current.status).toBe("idle");

    await act(async () => {
      resolve();
      await pending;
    });
    expect(result.current.status).toBe("copied");
  });

  it("goes failed with the reason when writeText rejects and the fallback fails, then idle after 3 s", async () => {
    setClipboard({ writeText: vi.fn().mockRejectedValue(new DOMException("denied")) });
    setExecCommand(vi.fn(() => false));
    const { result } = renderHook(() => useCopyToClipboard());

    await act(() => result.current.copy("text"));
    expect(result.current.status).toBe("failed");
    expect(result.current.error).toBe(CLIPBOARD_REFUSED);

    act(() => vi.advanceTimersByTime(FAILED_MS - 1));
    expect(result.current.status).toBe("failed");
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.status).toBe("idle");
    expect(result.current.error).toBeUndefined();
  });

  it("fails (never copied) when neither navigator.clipboard nor the fallback works", async () => {
    setClipboard(undefined);
    setExecCommand(vi.fn(() => false));
    const { result } = renderHook(() => useCopyToClipboard());

    await act(() => result.current.copy("text"));
    expect(result.current.status).toBe("failed");
    expect(result.current.error).toBe(CLIPBOARD_REFUSED);
  });

  it("copies through the execCommand fallback outside a secure context (no navigator.clipboard)", async () => {
    setClipboard(undefined);
    const execCommand = vi.fn(() => true);
    setExecCommand(execCommand);
    const { result } = renderHook(() => useCopyToClipboard());

    await act(() => result.current.copy("over-http"));
    expect(execCommand).toHaveBeenCalledWith("copy");
    expect(result.current.status).toBe("copied");
  });

  it("writes what the loader produces", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    const { result } = renderHook(() => useCopyToClipboard());

    await act(() => result.current.copy(() => Promise.resolve("loaded")));
    expect(writeText).toHaveBeenCalledWith("loaded");
    expect(result.current.status).toBe("copied");
  });

  it("fails without touching the clipboard when the loader rejects", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    const { result } = renderHook(() => useCopyToClipboard());

    await act(() => result.current.copy(() => Promise.reject(new Error("404"))));
    expect(writeText).not.toHaveBeenCalled();
    expect(result.current.status).toBe("failed");
    expect(result.current.error).toBe(LOAD_FAILED);
  });

  it("clears its timer on unmount", async () => {
    setClipboard({ writeText: vi.fn().mockResolvedValue(undefined) });
    const { result, unmount } = renderHook(() => useCopyToClipboard());
    await act(() => result.current.copy("text"));
    unmount();
    expect(vi.getTimerCount()).toBe(0);
  });
});
