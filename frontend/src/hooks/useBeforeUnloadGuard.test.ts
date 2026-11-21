import { renderHook } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import { useBeforeUnloadGuard } from "./useBeforeUnloadGuard";

function unload(): Event {
  const e = new Event("beforeunload", { cancelable: true });
  window.dispatchEvent(e);
  return e;
}

describe("useBeforeUnloadGuard (CORE-281)", () => {
  it("prevents the unload while active", () => {
    renderHook(() => useBeforeUnloadGuard(true));
    expect(unload().defaultPrevented).toBe(true);
  });

  it("lets the unload through while inactive", () => {
    renderHook(() => useBeforeUnloadGuard(false));
    expect(unload().defaultPrevented).toBe(false);
  });

  it("removes its listener when it turns inactive or unmounts", () => {
    const { rerender, unmount } = renderHook(({ active }) => useBeforeUnloadGuard(active), {
      initialProps: { active: true },
    });
    rerender({ active: false });
    expect(unload().defaultPrevented).toBe(false);
    rerender({ active: true });
    expect(unload().defaultPrevented).toBe(true);
    unmount();
    expect(unload().defaultPrevented).toBe(false);
  });
});
