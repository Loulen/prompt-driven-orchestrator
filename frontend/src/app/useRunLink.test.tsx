// CORE-1 — the deep link toward a Run (`#/runs/<id>`): from the address bar,
// from an extension's `CommandHost.open` and from a markdown link, it closes
// the open extension page and selects the Run; an unknown Run is a toast and
// changes nothing.
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import Markdown from "react-markdown";
import TopBar from "./TopBar";
import { useRunLink } from "./useRunLink";
import type { ExtensionEntry } from "../shared/generated/ExtensionEntry";
import type { UiContribution } from "../shared/generated/UiContribution";
import type { CommandHost } from "../shared/extensions/contract";
import ExtensionContextProvider from "../shared/extensions/ExtensionContextProvider";
import { followDeepLink, resetPages } from "../shared/extensions/deepLink";
import { hostReactMajor } from "../shared/extensions/shared";
import { resetExtensions, setBundleLoader, type BundleModule } from "../shared/extensions/store";
import { registerTransientOverlay } from "../lib/overlays";
import { MARKDOWN_COMPONENTS, REMARK_PLUGINS } from "../shared/markdown/plugins";
import { resetToasts } from "../shared/ui/toast";
import Toaster from "../shared/ui/Toaster";
import { TooltipProvider } from "../shared/ui/tooltip";

const contributions: UiContribution[] = [
  { location: "topbar/end", id: "home", label: "Demo", type: "page", component: "DemoPage" },
  { location: "topbar/end", id: "go", label: "Go to r1", type: "action", command: "demo.go" } as UiContribution,
  { location: "topbar/end", id: "ghost", label: "Go to ghost", type: "action", command: "demo.ghost" } as UiContribution,
];

const demo: ExtensionEntry = {
  id: "demo",
  name: "Extension demo",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/demo/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions,
    commands: [
      { id: "demo.go", title: "Go to r1", export: "go" },
      { id: "demo.ghost", title: "Go to ghost", export: "ghost" },
    ],
    locations: [],
  },
};

const bundle: BundleModule = {
  DemoPage: () => <p>the demo page</p>,
  go: (_context: unknown, host: CommandHost) => host.open("#/runs/r1"),
  ghost: (_context: unknown, host: CommandHost) => host.open("#/runs/ghost"),
};

function serve() {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url === "/ext") return Response.json([demo]);
      if (url === "/runs/r1") return Response.json({ run_id: "r1", pipeline_name: "demo", status: "running" });
      if (url === "/runs/ghost") return Response.json({ error: "run not found" }, { status: 404 });
      return new Response("not found", { status: 404 });
    }),
  );
}

/** The app shell's part of the link: TopBar hosts the extension pages, `useRunLink` the Runs. */
function Shell({ onSelect }: { onSelect: (runId: string) => void }) {
  const [selected, setSelected] = useState<string | null>("r0");
  useRunLink((runId) => {
    onSelect(runId);
    setSelected(runId);
  });
  return (
    <TooltipProvider>
      <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
        <TopBar onOpenSettings={() => {}} onOpenStats={() => {}} />
        <p data-testid="selected-run">{selected}</p>
        <Toaster />
      </ExtensionContextProvider>
    </TooltipProvider>
  );
}

beforeEach(() => {
  resetExtensions();
  resetPages();
  resetToasts();
  window.history.replaceState(null, "", "/");
  setBundleLoader(async () => bundle);
  serve();
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
  resetPages();
});

describe("#/runs/<id>", () => {
  it("in the address bar on load selects the Run, then leaves the address bar", async () => {
    window.history.replaceState(null, "", "/#/runs/r1");
    const onSelect = vi.fn();
    render(<Shell onSelect={onSelect} />);
    await waitFor(() => expect(screen.getByTestId("selected-run")).toHaveTextContent("r1"));
    expect(onSelect).toHaveBeenCalledWith("r1");
    expect(window.location.hash).toBe("");
  });

  it("opened by an extension's command closes its page and selects the Run", async () => {
    const onSelect = vi.fn();
    render(<Shell onSelect={onSelect} />);
    fireEvent.click(await screen.findByTestId("extension-topbar-end-demo:home"));
    expect(await screen.findByTestId("extension-page")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("extension-topbar-end-demo:go"));
    await waitFor(() => expect(screen.getByTestId("selected-run")).toHaveTextContent("r1"));
    expect(screen.queryByTestId("extension-page")).toBeNull();
    expect(window.location.hash).toBe("");
  });

  it("toward an unknown Run says so in a toast and changes nothing", async () => {
    const onSelect = vi.fn();
    render(<Shell onSelect={onSelect} />);
    fireEvent.click(await screen.findByTestId("extension-topbar-end-demo:home"));
    await screen.findByTestId("extension-page");

    fireEvent.click(screen.getByTestId("extension-topbar-end-demo:ghost"));
    expect(await screen.findByTestId("toast-error")).toHaveTextContent(
      "Run “ghost” not found: this link can't be opened.",
    );
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByTestId("selected-run")).toHaveTextContent("r0");
    // The page stays, and so does its own link in the address bar.
    expect(screen.getByTestId("extension-page")).toBeInTheDocument();
    expect(window.location.hash).toBe("#/ext/demo/topbar/end");
  });

  it("followed twice in a row selects the Run each time", async () => {
    const onSelect = vi.fn();
    render(<Shell onSelect={onSelect} />);
    await screen.findByTestId("extension-topbar-end-demo:home");
    act(() => followDeepLink("#/runs/r1"));
    await waitFor(() => expect(onSelect).toHaveBeenCalledTimes(1));
    act(() => followDeepLink("#/runs/r1"));
    await waitFor(() => expect(onSelect).toHaveBeenCalledTimes(2));
  });

  it("is clickable in a markdown render, and the viewer it sits in gives way", async () => {
    const onSelect = vi.fn();
    render(<Shell onSelect={onSelect} />);
    await screen.findByTestId("extension-topbar-end-demo:home");
    render(
      <Markdown remarkPlugins={REMARK_PLUGINS} components={MARKDOWN_COMPONENTS}>
        {"See [the run](#/runs/r1)."}
      </Markdown>,
    );
    const dismissViewer = vi.fn();
    const unregister = registerTransientOverlay(dismissViewer);
    fireEvent.click(screen.getByRole("link", { name: "the run" }));
    await waitFor(() => expect(onSelect).toHaveBeenCalledWith("r1"));
    expect(dismissViewer).toHaveBeenCalled();
    unregister();
  });
});
