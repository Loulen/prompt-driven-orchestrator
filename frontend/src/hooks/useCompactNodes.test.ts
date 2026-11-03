import { beforeEach, describe, expect, it, vi } from "vitest";
import { act, renderHook } from "@testing-library/react";

const saveAppearance = vi.hoisted(() => vi.fn(async (document: unknown) => ({ appearance: document })));
vi.mock("../settings/api/appearance", () => ({ fetchAppearance: vi.fn(), saveAppearance }));

import { resetCompactNodes, useCompactNodes } from "./useCompactNodes";

beforeEach(() => {
  localStorage.clear();
  resetCompactNodes();
  saveAppearance.mockClear();
});

describe("useCompactNodes", () => {
  it("is off by default", () => {
    const { result } = renderHook(() => useCompactNodes());
    expect(result.current.compactNodes).toBe(false);
  });

  it("starts on the stored choice, and reads an unknown value as off", () => {
    localStorage.setItem("pdo.ui.compactNodes", "true");
    expect(renderHook(() => useCompactNodes()).result.current.compactNodes).toBe(true);
    resetCompactNodes();
    localStorage.setItem("pdo.ui.compactNodes", "yes");
    expect(renderHook(() => useCompactNodes()).result.current.compactNodes).toBe(false);
  });

  it("persists a change in this browser and tells every reader", () => {
    const a = renderHook(() => useCompactNodes());
    const b = renderHook(() => useCompactNodes());
    act(() => a.result.current.toggleCompactNodes());
    expect(b.result.current.compactNodes).toBe(true);
    expect(localStorage.getItem("pdo.ui.compactNodes")).toBe("true");
    act(() => b.result.current.setCompactNodes(false));
    expect(a.result.current.compactNodes).toBe(false);
  });

  it("writes the Actor's Apparence, as the command field's switch does", () => {
    const { result } = renderHook(() => useCompactNodes());
    act(() => result.current.toggleCompactNodes());
    expect(saveAppearance).toHaveBeenCalledWith({ version: 1, compact_nodes: true, elements: {} });
  });

  it("follows a change made in another tab", () => {
    const { result } = renderHook(() => useCompactNodes());
    localStorage.setItem("pdo.ui.compactNodes", "true");
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "pdo.ui.compactNodes" }));
    });
    expect(result.current.compactNodes).toBe(true);
  });
});
