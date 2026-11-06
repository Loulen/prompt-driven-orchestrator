import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { openPage, resetPages, usePages } from "../shared/extensions/deepLink";
import { useSurfaces } from "./useSurfaces";

afterEach(() => {
  resetPages();
  window.history.replaceState(null, "", "/");
});

describe("useSurfaces and the pages of the Settings", () => {
  it("opens Settings, closing Stats, when a `settings/page` page opens", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.openStats());
    expect(result.current.statsOpen).toBe(true);
    act(() => openPage({ key: "test:settings-page", extension: "test", location: "settings/page", path: "" }));
    expect(result.current.settingsOpen).toBe(true);
    expect(result.current.statsOpen).toBe(false);
  });

  it("opens Settings at mount on a deep link already followed", () => {
    openPage({ key: "test:settings-page", extension: "test", location: "settings/page", path: "x" });
    const { result } = renderHook(() => useSurfaces());
    expect(result.current.settingsOpen).toBe(true);
  });

  it("leaves Settings closed for a full-window page", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => openPage({ key: "test:page", extension: "test", location: "topbar/end", path: "" }));
    expect(result.current.settingsOpen).toBe(false);
  });
});

describe("useSurfaces: one surface under the top bar at a time (CORE-183)", () => {
  const page = { key: "test:page", extension: "test", location: "topbar/end", path: "" } as const;

  it("closes Stats when a page of the top bar opens", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.openStats());
    act(() => openPage({ ...page, location: "topbar/start" }));
    expect(result.current.statsOpen).toBe(false);
  });

  it("closes the page of the top bar on screen when Settings or Stats opens", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => openPage(page));
    act(() => result.current.openSettings());
    expect(usePages.getState().open).toBeNull();
    expect(result.current.settingsOpen).toBe(true);
    act(() => result.current.settingsClosed());
    act(() => openPage(page));
    act(() => result.current.openStats());
    expect(usePages.getState().open).toBeNull();
    expect(result.current.statsOpen).toBe(true);
  });

  it("toggles Stats: a second click closes it", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.toggleStats());
    expect(result.current.statsOpen).toBe(true);
    act(() => result.current.toggleStats());
    expect(result.current.statsOpen).toBe(false);
  });

  it("toggles Settings: a second click asks the surface to close, through its own guard", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.toggleSettings());
    expect(result.current.settingsOpen).toBe(true);
    const before = result.current.settingsCloseRequest;
    act(() => result.current.toggleSettings());
    // The surface closes itself (its dirty draft may ask first): still open here.
    expect(result.current.settingsOpen).toBe(true);
    expect(result.current.settingsCloseRequest).toBe(before + 1);
    act(() => result.current.settingsClosed());
    expect(result.current.settingsOpen).toBe(false);
  });

  it("toggling Settings from Stats switches surfaces", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.toggleStats());
    act(() => result.current.toggleSettings());
    expect(result.current.statsOpen).toBe(false);
    expect(result.current.settingsOpen).toBe(true);
  });

  it("switches from Settings to Stats once Settings has closed through its guard", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.openSettings());
    const before = result.current.settingsCloseRequest;
    act(() => result.current.toggleStats());
    expect(result.current.settingsCloseRequest).toBe(before + 1);
    expect(result.current.statsOpen).toBe(false);
    act(() => result.current.settingsClosed());
    expect(result.current.settingsOpen).toBe(false);
    expect(result.current.statsOpen).toBe(true);
  });

  it("kept editing, Settings stays and the switch is undone", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.openSettings());
    act(() => result.current.toggleStats());
    act(() => result.current.settingsCloseCancelled());
    // Settings closed later by its ✕: Stats does not follow.
    act(() => result.current.settingsClosed());
    expect(result.current.statsOpen).toBe(false);
  });

  it("a page of the top bar replaces Settings through its guard; kept editing, the page closes", () => {
    const { result } = renderHook(() => useSurfaces());
    act(() => result.current.openSettings());
    const before = result.current.settingsCloseRequest;
    act(() => openPage(page));
    expect(result.current.settingsCloseRequest).toBe(before + 1);
    act(() => result.current.settingsCloseCancelled());
    expect(usePages.getState().open).toBeNull();
    expect(result.current.settingsOpen).toBe(true);
    // Answered by closing: the page stays.
    act(() => openPage(page));
    act(() => result.current.settingsClosed());
    expect(usePages.getState().open?.key).toBe("test:page");
    expect(result.current.settingsOpen).toBe(false);
  });

  it("closes every surface at once, then runs what waited for it", () => {
    const { result } = renderHook(() => useSurfaces());
    const then = vi.fn();
    act(() => result.current.openStats());
    act(() => result.current.closeSurfaces(then));
    expect(result.current.statsOpen).toBe(false);
    expect(then).toHaveBeenCalledTimes(1);
    act(() => openPage(page));
    act(() => result.current.closeSurfaces());
    expect(usePages.getState().open).toBeNull();
    // Over Settings, only once Settings is gone.
    act(() => result.current.openSettings());
    act(() => result.current.closeSurfaces(then));
    expect(then).toHaveBeenCalledTimes(1);
    act(() => result.current.settingsClosed());
    expect(then).toHaveBeenCalledTimes(2);
  });
});
