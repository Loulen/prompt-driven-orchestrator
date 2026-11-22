import { afterEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, useExtensionsStore } from "./store";

const ENTRY = {
  id: "test",
  name: "Test",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/test/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions: [],
    commands: [],
    locations: [],
  },
} as unknown as ExtensionEntry;

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the extensions store", () => {
  it("a load started before a reset neither imports with the next loader nor writes into the reset store", async () => {
    let answer: (r: Response) => void = () => {};
    vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>((resolve) => (answer = resolve))));
    const first = vi.fn(async () => ({ A: "first" }));
    setBundleLoader(first);
    const stale = useExtensionsStore.getState().load();

    resetExtensions();
    const next = vi.fn(async () => ({ A: "next" }));
    setBundleLoader(next);
    answer(new Response(JSON.stringify([ENTRY]), { status: 200, headers: { "Content-Type": "application/json" } }));
    await stale;

    expect(first).not.toHaveBeenCalled();
    expect(next).not.toHaveBeenCalled();
    expect(useExtensionsStore.getState()).toMatchObject({ extensions: [], started: false, loaded: false });
  });
});
