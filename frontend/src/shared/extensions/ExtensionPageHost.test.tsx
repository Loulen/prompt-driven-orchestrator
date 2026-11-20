/**
 * The full-window host only ever shows a *page* (CORE-165): a deep link toward
 * an extension that declares a panel before its page at `topbar/end` opens the
 * page, and an open entry naming a panel renders nothing full-window.
 */
import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiContribution } from "../generated/UiContribution";
import type { PageProps } from "./contract";
import { resetPages, usePages } from "./deepLink";
import ExtensionContextProvider from "./ExtensionContextProvider";
import ExtensionPageHost from "./ExtensionPageHost";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, useExtensionsLoaded } from "./store";

vi.mock("../../run/store/useDaemonSocket", () => ({ subscribe: () => () => {} }));

const contributions: UiContribution[] = [
  { location: "topbar/end", id: "inbox", label: "Inbox", type: "panel", component: "InboxPanel" },
  { location: "topbar/end", id: "pages", label: "Demo pages", type: "page", component: "PagesPage" },
];

const demo: ExtensionEntry = {
  id: "demo",
  name: "Demo",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/demo/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions,
    commands: [],
    locations: [],
  },
};

const bundle = {
  InboxPanel: () => <p>inbox panel</p>,
  PagesPage: ({ context }: PageProps) => <p>{`pages page at ${context.path}`}</p>,
};

beforeEach(() => {
  resetExtensions();
  resetPages();
  window.history.replaceState(null, "", "/");
  setBundleLoader(async () => bundle);
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext" ? Response.json([demo]) : new Response("not found", { status: 404 }),
    ),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
  resetPages();
  window.history.replaceState(null, "", "/");
});

/** The host, with a probe that says once every bundle is loaded. */
function Probe() {
  return useExtensionsLoaded() ? <span data-testid="loaded" /> : null;
}

function host() {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <ExtensionPageHost />
      <Probe />
    </ExtensionContextProvider>,
  );
}

describe("ExtensionPageHost", () => {
  it("opens the page, not the panel declared before it, on a deep link", async () => {
    window.history.replaceState(null, "", "/#/ext/demo/topbar/end/x");
    host();
    const page = await screen.findByTestId("extension-page");
    expect(page).toHaveTextContent("pages page at x");
    expect(page).toHaveTextContent("Demo pages");
    expect(screen.queryByText("inbox panel")).toBeNull();
  });

  it("renders nothing full-window when the open entry names a panel", async () => {
    host();
    await screen.findByTestId("loaded");
    act(() => {
      usePages.setState({ open: { key: "demo:inbox", extension: "demo", location: "topbar/end", path: "" } });
    });
    expect(screen.queryByTestId("extension-page")).toBeNull();
    expect(screen.queryByText("inbox panel")).toBeNull();
  });
});
