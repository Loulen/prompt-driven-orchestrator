/**
 * `app/banner` (0.31): the application banner, full-width lines under the top
 * bar, one per contribution in the catalogue's order; a contribution that
 * renders nothing takes no room, and nothing renders without a contribution.
 */

import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../shared/generated/ExtensionEntry";
import type { DecorationProps } from "../shared/extensions/contract";
import { APP_BANNER_HEIGHT_VAR } from "../shared/extensions/ExtensionAppBanner";
import { hostReactMajor } from "../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../shared/extensions/store";
import { Banner, Button } from "../shared/ui/kit";
import TopBar from "./TopBar";

function entry(id: string, contributions: unknown[]): ExtensionEntry {
  return {
    id,
    name: `Extension ${id}`,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands: [],
      locations: [],
    },
  } as ExtensionEntry;
}

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify(extensions), { status: 200 })
        : new Response("not found", { status: 404 }),
    ),
  );
}

const at = (id: string, component: string) => ({ location: "app/banner", id, label: id, type: "decoration", component });

const acted: string[] = [];

beforeEach(() => {
  resetExtensions();
  acted.length = 0;
  setBundleLoader(async () => ({
    Notice: ({ context, entity, extension }: DecorationProps) => (
      <Banner
        tone="awaiting"
        title={`${extension.id} notice`}
        text={`${context.location}:${entity === null ? "no item" : "item"}`}
        actions={
          <Button size="sm" onClick={() => acted.push(extension.id)}>
            Act
          </Button>
        }
      />
    ),
    Nothing: () => null,
    Boom: () => {
      throw new Error("boom");
    },
  }));
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
  document.documentElement.style.removeProperty(APP_BANNER_HEIGHT_VAR);
});

const renderBar = () => render(<TopBar onOpenSettings={() => {}} onOpenStats={() => {}} />);

describe("the app/banner location", () => {
  it("renders the banner under the top bar, with no entity, and its action works", async () => {
    serve([entry("a", [at("notice", "Notice")])]);
    renderBar();
    const banner = await screen.findByTestId("extension-app-banner-a:notice");
    expect(within(banner).getByRole("status")).toHaveTextContent("a notice");
    expect(banner).toHaveTextContent("app/banner:no item");
    // Under the bar, never inside it.
    const bar = screen.getByTestId("topbar");
    expect(bar.contains(banner)).toBe(false);
    expect(bar.compareDocumentPosition(banner) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(within(banner).getByRole("button", { name: "Act" }));
    expect(acted).toEqual(["a"]);
    // Nothing to dismiss it with: the action is the banner's only button.
    expect(within(banner).getAllByRole("button")).toHaveLength(1);
  });

  it("stacks the banners in the catalogue's order", async () => {
    serve([entry("a", [at("first", "Notice")]), entry("b", [at("second", "Notice")])]);
    renderBar();
    await screen.findByTestId("extension-app-banner-b:second");
    const rows = within(screen.getByTestId("extension-app-banner")).getAllByRole("status");
    expect(rows.map((r) => r.textContent?.slice(0, 8))).toEqual(["a notice", "b notice"]);
  });

  it("leaves an empty row, hidden by :empty, for a contribution that renders nothing", async () => {
    serve([entry("a", [at("nothing", "Nothing"), at("notice", "Notice")])]);
    renderBar();
    await screen.findByTestId("extension-app-banner-a:notice");
    const empty = screen.getByTestId("extension-app-banner-a:nothing");
    expect(empty.childNodes).toHaveLength(0);
    expect(empty.matches(":empty")).toBe(true);
    expect(empty).toHaveClass("sh-appbanner__item");
  });

  it("keeps a failing banner to its own line, naming its extension", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    serve([entry("a", [at("boom", "Boom"), at("notice", "Notice")])]);
    renderBar();
    await screen.findByTestId("extension-app-banner-a:notice");
    expect(screen.getByTestId("extension-app-banner-a:boom")).toHaveTextContent(
      "The banner of the extension Extension a failed",
    );
  });

  it("renders nothing, and publishes no height, without a contribution", async () => {
    serve([entry("a", [])]);
    renderBar();
    await act(async () => {});
    expect(screen.queryByTestId("extension-app-banner")).toBeNull();
    expect(document.documentElement.style.getPropertyValue(APP_BANNER_HEIGHT_VAR)).toBe("");
  });

  it("publishes its height for the full-window surfaces while it shows", async () => {
    serve([entry("a", [at("notice", "Notice")])]);
    const { unmount } = renderBar();
    await screen.findByTestId("extension-app-banner-a:notice");
    // jsdom lays nothing out: the height is 0 px, but it is published.
    expect(document.documentElement.style.getPropertyValue(APP_BANNER_HEIGHT_VAR)).toBe("0px");
    unmount();
    expect(document.documentElement.style.getPropertyValue(APP_BANNER_HEIGHT_VAR)).toBe("");
  });
});
