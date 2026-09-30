/**
 * The extensions of the daemon, loaded once per page: `GET /ext`, then each
 * declared UI bundle, imported at runtime.
 *
 * A bundle built for another major of React than the front's is never imported
 * (`incompatible`): mixing two Reacts breaks hooks in ways a banner explains
 * better than a crash. A bundle that fails to import is `failed`. Neither
 * affects the rest of the UI. A daemon without extension answers `[]`, and
 * nothing is added anywhere.
 */

import { useEffect } from "react";
import { create } from "zustand";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import { fetchExtensions } from "./api";
import { hostReactMajor } from "./shared";

/** What an ES bundle exports: its components, by name. */
export type BundleModule = Record<string, unknown>;
export type BundleLoader = (url: string) => Promise<BundleModule>;

const importBundle: BundleLoader = (url) => import(/* @vite-ignore */ url);
let loadBundle: BundleLoader = importBundle;

/** Replace how bundles are imported (tests); none restores the real `import()`. */
export function setBundleLoader(loader?: BundleLoader): void {
  loadBundle = loader ?? importBundle;
}

export type ExtensionStatus = "no-ui" | "loading" | "ready" | "incompatible" | "failed";

export interface LoadedExtension {
  entry: ExtensionEntry;
  status: ExtensionStatus;
  module: BundleModule | null;
}

interface ExtensionsState {
  extensions: LoadedExtension[];
  started: boolean;
  /** Every declared bundle was tried: each extension's status is final. */
  loaded: boolean;
  load: () => Promise<void>;
}

async function loadOne(entry: ExtensionEntry): Promise<LoadedExtension> {
  const url = entry.ui?.bundle_url;
  if (!url) return { entry, status: "no-ui", module: null };
  if (entry.ui?.react_major !== hostReactMajor()) {
    return { entry, status: "incompatible", module: null };
  }
  try {
    return { entry, status: "ready", module: await loadBundle(url) };
  } catch (cause) {
    console.warn(`extension ${entry.id}: its UI bundle ${url} did not load`, cause);
    return { entry, status: "failed", module: null };
  }
}

export const useExtensionsStore = create<ExtensionsState>((set, get) => ({
  extensions: [],
  started: false,
  loaded: false,
  load: async () => {
    if (get().started) return;
    set({ started: true });
    let entries: ExtensionEntry[];
    try {
      entries = await fetchExtensions();
      if (!Array.isArray(entries)) entries = [];
    } catch {
      // No `/ext` (an older daemon, a test double): no extension.
      entries = [];
    }
    set({ extensions: entries.map((entry) => ({ entry, status: "loading", module: null })) });
    const loaded = await Promise.all(entries.map(loadOne));
    set({ extensions: loaded, loaded: true });
  },
}));

/** Forget what was loaded (tests). */
export function resetExtensions(): void {
  useExtensionsStore.setState({ extensions: [], started: false, loaded: false });
  setBundleLoader();
}

/** The extensions, loading them on first use. */
export function useExtensions(): LoadedExtension[] {
  const extensions = useExtensionsStore((s) => s.extensions);
  useEffect(() => {
    void useExtensionsStore.getState().load();
  }, []);
  return extensions;
}

/** Whether every extension's status is final (loading them on first use). */
export function useExtensionsLoaded(): boolean {
  const loaded = useExtensionsStore((s) => s.loaded);
  useEffect(() => {
    void useExtensionsStore.getState().load();
  }, []);
  return loaded;
}
