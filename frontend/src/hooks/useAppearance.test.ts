import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ActorAppearance } from "../shared/generated/ActorAppearance";
import type { AppearanceDocument } from "../shared/generated/AppearanceDocument";

const api = vi.hoisted(() => ({
  fetchAppearance: vi.fn<() => Promise<ActorAppearance>>(),
  saveAppearance: vi.fn<(document: AppearanceDocument) => Promise<ActorAppearance>>(),
}));
vi.mock("../settings/api/appearance", () => api);

import { initAppearance, resetAppearance, syncAppearance, updateAppearance, useAppearance } from "./useAppearance";

const painted = () => [document.documentElement.getAttribute("data-theme"), document.documentElement.getAttribute("data-mode")];

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  api.fetchAppearance.mockReset();
  api.saveAppearance.mockReset().mockImplementation(async (document) => ({ appearance: document }));
});

afterEach(() => resetAppearance());

describe("the Apparence store", () => {
  it("paints this browser's copy at boot, before the daemon answers", () => {
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.mode", "light");
    initAppearance();
    expect(painted()).toEqual(["classic", "light"]);
    expect(renderHook(() => useAppearance()).result.current.theme).toBe("classic");
  });

  it("adopts the daemon's document and keeps it as the local copy", async () => {
    initAppearance();
    api.fetchAppearance.mockResolvedValue({
      appearance: { version: 1, theme: "classic", dark_mode: false, compact_nodes: true, elements: {} },
    });
    const { result } = renderHook(() => useAppearance());
    await act(() => syncAppearance());
    expect(result.current).toMatchObject({ theme: "classic", mode: "light", compactNodes: true });
    expect(painted()).toEqual(["classic", "light"]);
    expect(localStorage.getItem("pdo.ui.theme")).toBe("classic");
    expect(localStorage.getItem("pdo.ui.mode")).toBe("light");
    expect(localStorage.getItem("pdo.ui.compactNodes")).toBe("true");
    expect(api.saveAppearance).not.toHaveBeenCalled();
  });

  it("gives an Actor without an Apparence the values this browser held", async () => {
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.compactNodes", "true");
    initAppearance();
    api.fetchAppearance.mockResolvedValue({ appearance: null });
    await syncAppearance();
    expect(api.saveAppearance).toHaveBeenCalledWith({ version: 1, theme: "classic", compact_nodes: true, elements: {} });
  });

  it("writes a change at once, here and on the daemon, the latest one when they queue", async () => {
    initAppearance();
    let release = () => {};
    api.saveAppearance.mockImplementationOnce(
      (document) => new Promise((resolve) => (release = () => resolve({ appearance: document }))),
    );
    const { result } = renderHook(() => useAppearance());
    act(() => updateAppearance((a) => ({ ...a, compactNodes: true })));
    act(() => updateAppearance((a) => ({ ...a, mode: "light" })));
    act(() => updateAppearance((a) => ({ ...a, theme: "classic" })));
    expect(result.current).toMatchObject({ theme: "classic", mode: "light", compactNodes: true });
    expect(localStorage.getItem("pdo.ui.mode")).toBe("light");
    await act(async () => release());
    await vi.waitFor(() => expect(api.saveAppearance).toHaveBeenCalledTimes(2));
    expect(api.saveAppearance.mock.calls[1][0]).toEqual({
      version: 1,
      theme: "classic",
      dark_mode: false,
      compact_nodes: true,
      elements: {},
    });
  });

  it("keeps a change made while the daemon answered over the daemon's answer", async () => {
    initAppearance();
    let answer: (value: ActorAppearance) => void = () => {};
    api.fetchAppearance.mockReturnValue(new Promise<ActorAppearance>((resolve) => (answer = resolve)));
    const sync = syncAppearance();
    updateAppearance((a) => ({ ...a, theme: "classic" }));
    answer({ appearance: { version: 1, elements: {} } });
    await sync;
    const { result } = renderHook(() => useAppearance());
    expect(result.current.theme).toBe("classic");
  });

  it("stays on the local copy when the daemon does not answer", async () => {
    localStorage.setItem("pdo.ui.mode", "light");
    initAppearance();
    api.fetchAppearance.mockRejectedValue(new Error("down"));
    await syncAppearance();
    expect(painted()).toEqual(["acogni", "light"]);
    expect(api.saveAppearance).not.toHaveBeenCalled();
  });

  it("follows another tab of this browser at once, without writing to the daemon", () => {
    initAppearance();
    const { result } = renderHook(() => useAppearance());
    localStorage.setItem("pdo.ui.theme", "classic");
    localStorage.setItem("pdo.ui.appearance.elements", JSON.stringify({ "runs.list.cost": true }));
    act(() => {
      window.dispatchEvent(new StorageEvent("storage", { key: "pdo.ui.appearance.elements" }));
    });
    expect(result.current.theme).toBe("classic");
    expect(result.current.elements).toEqual({ "runs.list.cost": true });
    expect(painted()[0]).toBe("classic");
    expect(api.saveAppearance).not.toHaveBeenCalled();
  });
});

describe("the Apparence store, against a write the daemon has not confirmed", () => {
  it("writes a refused change again at the next sync instead of adopting the daemon's older document", async () => {
    initAppearance();
    api.saveAppearance.mockRejectedValueOnce(new Error("restarting"));
    updateAppearance((a) => ({ ...a, theme: "classic" }));
    await vi.waitFor(() => expect(api.saveAppearance).toHaveBeenCalledTimes(1));
    api.fetchAppearance.mockResolvedValue({ appearance: { version: 1, elements: {} } });
    await syncAppearance();
    expect(api.fetchAppearance).not.toHaveBeenCalled();
    expect(api.saveAppearance).toHaveBeenCalledTimes(2);
    expect(api.saveAppearance.mock.calls[1][0]).toMatchObject({ theme: "classic" });
    expect(localStorage.getItem("pdo.ui.theme")).toBe("classic");
    // Saved now: the next sync reads the daemon again.
    await syncAppearance();
    expect(api.fetchAppearance).toHaveBeenCalledTimes(1);
  });

  it("drops a daemon answer older than a change another tab made meanwhile", async () => {
    initAppearance();
    let answer: (value: ActorAppearance) => void = () => {};
    api.fetchAppearance.mockReturnValue(new Promise<ActorAppearance>((resolve) => (answer = resolve)));
    const sync = syncAppearance();
    localStorage.setItem("pdo.ui.theme", "classic");
    window.dispatchEvent(new StorageEvent("storage", { key: "pdo.ui.theme" }));
    answer({ appearance: { version: 1, elements: {} } });
    await sync;
    expect(localStorage.getItem("pdo.ui.theme")).toBe("classic");
    expect(renderHook(() => useAppearance()).result.current.theme).toBe("classic");
  });
});
