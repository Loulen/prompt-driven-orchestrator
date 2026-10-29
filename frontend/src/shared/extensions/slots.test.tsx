import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useState } from "react";
import TopBar from "../../app/TopBar";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { Attributes } from "./attributes";
import ExtensionDetailTab from "./ExtensionDetailTab";
import ExtensionListFilters from "./ExtensionListFilters";
import ExtensionSettingsPanels from "../../settings/components/settingsSurface/ExtensionPanels";
import { useDetailTabs } from "./catalogue";
import type { ListFilterProps, TabProps } from "./contract";
import { resetPages } from "./deepLink";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, type BundleModule } from "./store";

// The four locations of version 0.1 (#931), as an extension written for 0.1
// fills them: the daemon maps its four slots onto `topbar/start`, `settings`,
// `runs/filter` and `run/detail` (`UiSlot::contribution`), and they must show
// as before (#29).

/** The test extension of the daemon's suite, as `GET /ext` declares its 0.1 slots. */
function testExtension(reactMajor = hostReactMajor()): ExtensionEntry {
  return {
    id: "test",
    name: "Test extension",
    version: "0.0.0",
    ui: {
      bundle_url: "/ext/test/ui.js",
      react_major: reactMajor,
      ui_module_major: null,
      locations: [],
      contributions: [
        { location: "topbar/start", id: "events", label: "Test events", type: "page", component: "EventsPage" },
        { location: "settings", id: "settings", label: "Test extension", type: "section", component: "SettingsPanel" },
        {
          location: "runs/filter",
          id: "started-by",
          label: "Started by",
          type: "list_filter",
          component: "StartedByFilter",
        },
        { location: "run/detail", id: "run-tab", label: "Test", type: "tab", component: "RunTab" },
      ],
      commands: [],
    },
  };
}

/** What the test bundle exports: one component per slot. */
const bundle: BundleModule = {
  EventsPage: () => <p>events page of the extension</p>,
  SettingsPanel: () => <p>settings panel of the extension</p>,
  StartedByFilter: ({ filter, setFilter }: ListFilterProps) => (
    <button type="button" onClick={() => setFilter({ ...filter, "test/started_by": "alice" })}>
      only alice ({filter["test/started_by"] ?? "anyone"})
    </button>
  ),
  RunTab: ({ entity }: TabProps) => (
    <p>
      tab of {entity.kind} {entity.id}: {entity.attributes["test/started_by"]}
    </p>
  ),
};

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url === "/ext") {
        return new Response(JSON.stringify(extensions), {
          status: 200,
          headers: { "Content-Type": "application/json" },
        });
      }
      return new Response("not found", { status: 404 });
    }),
  );
}

let loader: ReturnType<typeof vi.fn>;

beforeEach(() => {
  resetExtensions();
  resetPages();
  window.history.replaceState(null, "", "/");
  loader = vi.fn(async () => bundle);
  setBundleLoader(loader as unknown as (url: string) => Promise<BundleModule>);
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

function renderTopBar() {
  return render(<TopBar onOpenSettings={() => {}} onOpenStats={() => {}} />);
}

describe("the UI slots", () => {
  it("adds a navigation entry that opens the extension's page", async () => {
    serve([testExtension()]);
    renderTopBar();
    const entry = await screen.findByTestId("extension-nav-test:events");
    expect(entry).toHaveTextContent("Test events");
    expect(loader).toHaveBeenCalledWith("/ext/test/ui.js");
    fireEvent.click(entry);
    expect(await screen.findByTestId("extension-page")).toBeInTheDocument();
    expect(screen.getByText("events page of the extension")).toBeInTheDocument();
  });

  it("adds a settings panel", async () => {
    serve([testExtension()]);
    render(<ExtensionSettingsPanels active />);
    expect(await screen.findByText("settings panel of the extension")).toBeInTheDocument();
    const section = screen.getByTestId("extension-settings-test:settings");
    expect(section).toHaveTextContent("Test extension");
    expect(section.querySelector('[title="Added by the extension Test extension 0.0.0"]')).not.toBeNull();
  });

  it("adds a list filter that sets the attribute filter", async () => {
    serve([testExtension()]);
    function Host() {
      const [filter, setFilter] = useState<Attributes>({});
      return (
        <>
          <ExtensionListFilters location="runs/filter" filter={filter} onChange={setFilter} />
          <output data-testid="filter">{JSON.stringify(filter)}</output>
        </>
      );
    }
    render(<Host />);
    const control = await screen.findByText("only alice (anyone)");
    fireEvent.click(control);
    expect(screen.getByTestId("filter")).toHaveTextContent('{"test/started_by":"alice"}');
    expect(screen.getByText("only alice (alice)")).toBeInTheDocument();
  });

  it("adds a tab to the detail of an entity", async () => {
    serve([testExtension()]);
    const entity = { kind: "run", id: "r1", attributes: { "test/started_by": "alice" } };
    function Detail() {
      const { tabs, context } = useDetailTabs(entity);
      const [tab] = tabs;
      if (!tab) return null;
      return <ExtensionDetailTab tab={tab} context={context} />;
    }
    render(<Detail />);
    expect(await screen.findByText("tab of run r1: alice")).toBeInTheDocument();
  });

  it("keeps a failing extension component to its own element", async () => {
    serve([testExtension()]);
    setBundleLoader(async () => ({
      ...bundle,
      SettingsPanel: () => {
        throw new Error("boom");
      },
    }));
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    render(<ExtensionSettingsPanels active />);
    expect(await screen.findByTestId("extension-failed-test")).toBeInTheDocument();
    warn.mockRestore();
    error.mockRestore();
  });
});

describe("an incompatible bundle", () => {
  it("is not loaded, and the banner says so while the rest of the UI works", async () => {
    serve([testExtension(hostReactMajor() - 1)]);
    renderTopBar();
    const banner = await screen.findByTestId("extension-banner-test");
    expect(banner).toHaveTextContent(
      `The interface of the extension Test extension was not loaded: it was built for React ${hostReactMajor() - 1}, and the app runs React ${hostReactMajor()}.`,
    );
    expect(loader).not.toHaveBeenCalled();
    expect(screen.queryByTestId("extension-nav")).toBeNull();
    expect(screen.getByTestId("open-settings")).toBeInTheDocument();
  });
});

describe("without extension", () => {
  it("adds no element anywhere", async () => {
    serve([]);
    const { container } = renderTopBar();
    await act(async () => {
      await Promise.resolve();
    });
    await waitFor(() => expect(fetch).toHaveBeenCalledWith("/ext", expect.anything()));
    expect(screen.queryByTestId("extension-nav")).toBeNull();
    expect(screen.queryByTestId("extension-banner")).toBeNull();
    // The core's own: the command field, Stats, Settings, the theme.
    expect(container.querySelectorAll("header button")).toHaveLength(4);
    expect(screen.queryByTestId("extension-topbar-end")).toBeNull();
    expect(loader).not.toHaveBeenCalled();
  });

  it("treats a daemon without /ext as one without extension", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("nope", { status: 404 })));
    renderTopBar();
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(screen.queryByTestId("extension-nav")).toBeNull();
    expect(screen.queryByTestId("extension-banner")).toBeNull();
  });
});
