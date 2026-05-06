import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, beforeEach } from "vitest";
import {
  useResizableLayout,
  clampLayout,
  type Layout,
} from "./useResizableLayout";

const PANEL_IDS = ["left", "center", "right"];

function makeLayout(left: number, center: number, right: number): Layout {
  return { left, center, right };
}

beforeEach(() => {
  localStorage.clear();
});

describe("clampLayout", () => {
  it("passes through sizes above minimum", () => {
    const result = clampLayout(makeLayout(20, 60, 20), 10);
    expect(result).toEqual(makeLayout(20, 60, 20));
  });

  it("clamps sizes below minimum and redistributes", () => {
    const result = clampLayout(makeLayout(3, 90, 7), 10);
    expect(result.left).toBeGreaterThanOrEqual(10);
    expect(result.right).toBeGreaterThanOrEqual(10);
    const sum = Object.values(result).reduce((a, b) => a + b, 0);
    expect(sum).toBeCloseTo(100, 0);
  });

  it("clamps all panels when all are below minimum", () => {
    const result = clampLayout(makeLayout(2, 3, 2), 10);
    Object.values(result).forEach((s) =>
      expect(s).toBeGreaterThanOrEqual(10),
    );
    const sum = Object.values(result).reduce((a, b) => a + b, 0);
    expect(sum).toBeCloseTo(100, 0);
  });
});

describe("useResizableLayout", () => {
  const defaults = makeLayout(15, 60, 25);

  it("returns default sizes when localStorage is empty", () => {
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    expect(result.current.defaultLayout).toEqual(defaults);
  });

  it("persists layout to localStorage on change", () => {
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );

    act(() => {
      result.current.onLayoutChanged(makeLayout(20, 55, 25));
    });

    const stored = JSON.parse(
      localStorage.getItem("maestro.layout.run") ?? "null",
    );
    expect(stored).toEqual(makeLayout(20, 55, 25));
  });

  it("restores layout from localStorage on mount", () => {
    localStorage.setItem(
      "maestro.layout.run",
      JSON.stringify(makeLayout(25, 50, 25)),
    );
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    expect(result.current.defaultLayout).toEqual(makeLayout(25, 50, 25));
  });

  it("uses separate keys for run and edit modes", () => {
    localStorage.setItem(
      "maestro.layout.run",
      JSON.stringify(makeLayout(20, 55, 25)),
    );
    localStorage.setItem(
      "maestro.layout.edit",
      JSON.stringify(makeLayout(30, 40, 30)),
    );

    const { result: runResult } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    const { result: editResult } = renderHook(() =>
      useResizableLayout("edit", PANEL_IDS, defaults),
    );

    expect(runResult.current.defaultLayout).toEqual(makeLayout(20, 55, 25));
    expect(editResult.current.defaultLayout).toEqual(makeLayout(30, 40, 30));
  });

  it("does not cross-contaminate modes on save", () => {
    const { result: runHook } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    const { result: editHook } = renderHook(() =>
      useResizableLayout("edit", PANEL_IDS, defaults),
    );

    act(() => {
      runHook.current.onLayoutChanged(makeLayout(22, 53, 25));
    });

    expect(localStorage.getItem("maestro.layout.run")).toBeTruthy();
    expect(localStorage.getItem("maestro.layout.edit")).toBeNull();

    act(() => {
      editHook.current.onLayoutChanged(makeLayout(30, 40, 30));
    });

    expect(
      JSON.parse(localStorage.getItem("maestro.layout.run")!),
    ).toEqual(makeLayout(22, 53, 25));
    expect(
      JSON.parse(localStorage.getItem("maestro.layout.edit")!),
    ).toEqual(makeLayout(30, 40, 30));
  });

  it("clamps stored sizes below minimum percentage", () => {
    localStorage.setItem(
      "maestro.layout.run",
      JSON.stringify(makeLayout(2, 94, 4)),
    );
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    Object.values(result.current.defaultLayout).forEach((s) =>
      expect(s).toBeGreaterThanOrEqual(5),
    );
  });

  it("ignores malformed localStorage data", () => {
    localStorage.setItem("maestro.layout.run", "not-json");
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    expect(result.current.defaultLayout).toEqual(defaults);
  });

  it("ignores stored object with missing panel ids", () => {
    localStorage.setItem(
      "maestro.layout.run",
      JSON.stringify({ left: 50, center: 50 }),
    );
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    expect(result.current.defaultLayout).toEqual(defaults);
  });

  it("exposes MIN_SIZE_PX as 100", () => {
    const { result } = renderHook(() =>
      useResizableLayout("run", PANEL_IDS, defaults),
    );
    expect(result.current.minSizePx).toBe(100);
  });
});
