/**
 * The extensions store: a load started before `resetExtensions` writes nothing
 * after it, so a test's in-flight `GET /ext` never leaks into the next one.
 */

import { afterEach, describe, expect, it, vi } from "vitest";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, useExtensionsStore } from "./store";

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

const ENTRY = {
  id: "stale",
  name: "Stale",
  version: "1.0.0",
  ui: { bundle_url: "/ext/stale/ui.js", react_major: hostReactMajor(), ui_module_major: null, contributions: [], commands: [], locations: [] },
};

describe("the extensions store", () => {
  it("drops a load that a reset overtook", async () => {
    let answer: (r: Response) => void = () => {};
    vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>((resolve) => (answer = resolve))));
    setBundleLoader(async () => ({}));
    const stale = useExtensionsStore.getState().load();
    resetExtensions();
    answer(new Response(JSON.stringify([ENTRY]), { status: 200 }));
    await stale;
    expect(useExtensionsStore.getState()).toMatchObject({ extensions: [], started: false, loaded: false });
  });

  it("keeps a load that no reset overtook", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify([ENTRY]), { status: 200 })));
    setBundleLoader(async () => ({}));
    await useExtensionsStore.getState().load();
    expect(useExtensionsStore.getState().extensions.map((e) => [e.entry.id, e.status])).toEqual([["stale", "ready"]]);
  });
});
