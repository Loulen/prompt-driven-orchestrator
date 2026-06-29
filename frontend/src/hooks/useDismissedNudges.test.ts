import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import { useDismissedNudges } from "./useDismissedNudges";

beforeEach(() => {
  localStorage.clear();
});

describe("useDismissedNudges (#268)", () => {
  it("starts empty when nothing is persisted", () => {
    const { result } = renderHook(() => useDismissedNudges("p1"));
    expect(result.current.dismissed).toEqual(new Set());
  });

  it("dismiss adds to the set and persists to localStorage", () => {
    const { result } = renderHook(() => useDismissedNudges("p1"));

    act(() => result.current.dismiss("fanout:worker"));

    expect(result.current.dismissed).toEqual(new Set(["fanout:worker"]));
    expect(
      JSON.parse(localStorage.getItem("pdo.banner.dismissed.p1") ?? "null"),
    ).toEqual(["fanout:worker"]);
  });

  it("dismissing the same id twice is a stable no-op (same set reference)", () => {
    const { result } = renderHook(() => useDismissedNudges("p1"));

    act(() => result.current.dismiss("fanout:worker"));
    const first = result.current.dismissed;
    act(() => result.current.dismiss("fanout:worker"));

    expect(result.current.dismissed).toBe(first);
  });

  it("a remount reads the persisted set", () => {
    localStorage.setItem(
      "pdo.banner.dismissed.p1",
      JSON.stringify(["fanout:worker"]),
    );
    const { result } = renderHook(() => useDismissedNudges("p1"));
    expect(result.current.dismissed).toEqual(new Set(["fanout:worker"]));
  });

  it("reloads a different pipeline's set when tabId changes", () => {
    localStorage.setItem("pdo.banner.dismissed.p1", JSON.stringify(["fanout:a"]));
    localStorage.setItem("pdo.banner.dismissed.p2", JSON.stringify(["fanout:b"]));

    const { result, rerender } = renderHook(({ id }) => useDismissedNudges(id), {
      initialProps: { id: "p1" },
    });
    expect(result.current.dismissed).toEqual(new Set(["fanout:a"]));

    rerender({ id: "p2" });
    expect(result.current.dismissed).toEqual(new Set(["fanout:b"]));
  });

  it("does not cross-contaminate pipelines on dismiss", () => {
    const { result } = renderHook(() => useDismissedNudges("p1"));
    act(() => result.current.dismiss("fanout:a"));

    expect(localStorage.getItem("pdo.banner.dismissed.p2")).toBeNull();
  });
});
