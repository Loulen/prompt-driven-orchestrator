import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { openPage, resetPages } from "../shared/extensions/deepLink";
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
