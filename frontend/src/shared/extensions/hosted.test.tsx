import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useMemo } from "react";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { HostedLocation } from "../generated/HostedLocation";
import type { RunState } from "../generated/RunState";
import type { UiCommand } from "../generated/UiCommand";
import type { UiContribution } from "../generated/UiContribution";
import { resetToasts } from "../ui/toast";
import Toaster from "../ui/Toaster";
import type { CommandContext, CommandHost } from "./contract";
import ExtensionBanner from "./ExtensionBanner";
import ExtensionContextProvider from "./ExtensionContextProvider";
import { useHostedContributions, type HostContext } from "./hosted";
import { HostedContribution, HostedContributions } from "./HostedContributions";
import { extensionUiModule } from "./uiModule";
import { hostReactMajor, shareWithExtensions } from "./shared";
import { resetExtensions, setBundleLoader, type BundleModule } from "./store";
import { UI_MODULE_MAJOR } from "./uiModuleVersion";

const PANEL: HostedLocation = {
  id: "a/panneau",
  accepts: "section",
  description: "",
  context: [{ key: "item", description: "{ id, title }" }],
};
const ACTIONS: HostedLocation = { id: "a/actions", accepts: "action", description: "", context: [] };

function extension(
  id: string,
  contributions: UiContribution[] = [],
  options: { locations?: HostedLocation[]; commands?: UiCommand[]; uiModuleMajor?: number | null } = {},
): ExtensionEntry {
  return {
    id,
    name: `Extension ${id}`,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: options.uiModuleMajor ?? null,
      contributions,
      commands: options.commands ?? [],
      locations: options.locations ?? [],
    },
  };
}

const host = (uiModuleMajor: number = UI_MODULE_MAJOR) =>
  extension("a", [], { locations: [PANEL, ACTIONS], uiModuleMajor });

function section(id: string, component: string, when?: UiContribution["when"]): UiContribution {
  return { location: "a/panneau", id, label: `Section ${id}`, type: "section", component, ...(when ? { when } : {}) };
}

const calls: CommandContext[] = [];

const bundles: Record<string, BundleModule> = {
  "/ext/a/ui.js": {},
  "/ext/b/ui.js": {
    Panel: ({ extension, context }: { extension: { id: string }; context: HostContext }) => (
      <p>
        {extension.id} sees {(context.item as { title: string }).title} in {String(context.location)} on run{" "}
        {(context.run as { id: string } | null)?.id ?? "none"}
      </p>
    ),
    Second: () => <p>second of b</p>,
    Broken: () => {
      throw new Error("boom");
    },
    ping: (context: CommandContext, commandHost: CommandHost) => {
      calls.push(context);
      commandHost.notify(`pinged on ${(context as unknown as HostContext & { item: { id: string } }).item.id}`);
    },
  },
  "/ext/c/ui.js": { Panel: () => <p>c too</p> },
};

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify(extensions), { status: 200, headers: { "Content-Type": "application/json" } })
        : new Response("not found", { status: 404 }),
    ),
  );
}

/** What A's bundle does: render its location under an item. */
function HostItem({ location = "a/panneau" }: { location?: string }) {
  const context = useMemo(() => ({ item: { id: "i1", title: "First item" } }), []);
  return (
    <section data-testid="host-item">
      <HostedContributions location={location} context={context} />
    </section>
  );
}

const run = { run_id: "r1", pipeline_name: "demo", status: "running", name: null } as unknown as RunState;

function renderHost(ui = <HostItem />) {
  return render(
    <ExtensionContextProvider selectedRun={run} runs={[]} projects={[]}>
      {ui}
      <ExtensionBanner />
      <Toaster />
    </ExtensionContextProvider>,
  );
}

beforeEach(() => {
  resetExtensions();
  resetToasts();
  calls.length = 0;
  setBundleLoader(async (url) => bundles[url] ?? {});
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the shared module @pdo/extension-ui", () => {
  it("is what the import map's shim re-exports", async () => {
    shareWithExtensions(extensionUiModule);
    const shim = await import("../../../public/shared/pdo-extension-ui.js");
    expect(shim.default).toBe(extensionUiModule);
    for (const name of Object.keys(extensionUiModule)) {
      expect(shim[name as keyof typeof shim], name).toBe(extensionUiModule[name as keyof typeof extensionUiModule]);
    }
  });

  it("renders every extension's contributions to a hosted location, with the host's context", async () => {
    serve([
      extension("b", [section("one", "Panel"), section("two", "Second")]),
      host(),
      extension("c", [section("three", "Panel")]),
    ]);
    renderHost();
    const item = screen.getByTestId("host-item");
    expect(await within(item).findByText("b sees First item in a/panneau on run r1")).toBeInTheDocument();
    expect(within(item).getByText("second of b")).toBeInTheDocument();
    expect(within(item).getByText("c too")).toBeInTheDocument();
    expect(within(item).getAllByTestId(/^hosted-/).map((e) => e.dataset.testid)).toEqual([
      "hosted-b:one",
      "hosted-b:two",
      "hosted-c:three",
    ]);
  });

  it("hides an element whose when clause is false on the host's context", async () => {
    serve([
      extension("b", [
        section("shown", "Second", { eq: ["item.id", "i1"] }),
        section("hidden", "Panel", { eq: ["item.id", "i2"] }),
      ]),
      host(),
    ]);
    renderHost();
    expect(await screen.findByText("second of b")).toBeInTheDocument();
    expect(screen.queryByTestId("hosted-b:hidden")).toBeNull();
  });

  it("renders nothing without contribution, and for a location no loaded extension hosts", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    serve([extension("b", [{ ...section("lost", "Panel"), location: "gone/panel" }]), host()]);
    renderHost(
      <>
        <HostItem />
        <HostItem location="gone/panel" />
      </>,
    );
    await waitFor(() => expect(warn).toHaveBeenCalledWith(expect.stringContaining("gone/panel")));
    for (const item of screen.getAllByTestId("host-item")) expect(item).toBeEmptyDOMElement();
    warn.mockRestore();
  });

  it("keeps a failing element to itself", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    serve([extension("b", [section("broken", "Broken"), section("fine", "Second")]), host()]);
    renderHost();
    expect(await screen.findByTestId("extension-failed-b")).toBeInTheDocument();
    expect(screen.getByText("second of b")).toBeInTheDocument();
    warn.mockRestore();
    error.mockRestore();
  });

  it("renders an action as a button that calls its command with the host's context", async () => {
    const ping = { id: "b.ping", title: "Ping", export: "ping" };
    serve([
      extension("b", [{ location: "a/actions", id: "ping", label: "Ping", type: "action", command: "b.ping" }], {
        commands: [ping],
      }),
      host(),
    ]);
    function Header() {
      const context = useMemo(() => ({ item: { id: "i1" } }), []);
      const elements = useHostedContributions("a/actions", context);
      return (
        <div data-testid="header">
          {elements.map((element) => (
            <HostedContribution key={element.key} element={element} />
          ))}
        </div>
      );
    }
    renderHost(<Header />);
    fireEvent.click(await screen.findByTestId("hosted-action-b:ping"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({ location: "a/actions", item: { id: "i1" }, run: { id: "r1" }, contribution: "ping" });
    expect(await screen.findByText("pinged on i1")).toBeInTheDocument();
  });

  it("does not load a bundle built against another major of the module, and says so", async () => {
    const loader = vi.fn(async (url: string) => bundles[url] ?? {});
    setBundleLoader(loader);
    serve([host(UI_MODULE_MAJOR + 1), extension("b", [section("one", "Panel")])]);
    renderHost();
    const banner = await screen.findByTestId("extension-banner-a");
    expect(banner).toHaveTextContent(
      `The interface of the extension Extension a was not loaded: it was built for the PDO extension UI module ${UI_MODULE_MAJOR + 1}, and PDO runs version ${UI_MODULE_MAJOR}.`,
    );
    await waitFor(() => expect(loader).toHaveBeenCalledWith("/ext/b/ui.js"));
    expect(loader).not.toHaveBeenCalledWith("/ext/a/ui.js");
  });
});
