/**
 * A deep link opens the extension's first *page* at its location (CORE-165):
 * a panel or an action declared there before the page is never opened by a
 * link, and an extension with no page there shows the « no page » error.
 */
import { describe, expect, it } from "vitest";
import type { UiCommand } from "../generated/UiCommand";
import type { UiContribution } from "../generated/UiContribution";
import type { DeepLink } from "./deepLink";
import { resolveDeepLink } from "./resolveDeepLink";
import type { BundleModule, LoadedExtension } from "./store";

const module: BundleModule = {
  InboxPanel: () => null,
  PagesPage: () => null,
  APage: () => null,
  BPage: () => null,
  sync: () => {},
};

function demo(
  contributions: UiContribution[],
  { commands = [], status = "ready" }: { commands?: UiCommand[]; status?: LoadedExtension["status"] } = {},
): LoadedExtension {
  return {
    entry: {
      id: "demo",
      name: "Demo",
      version: "1.0.0",
      ui: { bundle_url: "/ext/demo/ui.js", react_major: 19, ui_module_major: null, contributions, commands, locations: [] },
    },
    status,
    module: status === "ready" ? module : null,
  };
}

const panel: UiContribution = { location: "topbar/end", id: "inbox", label: "Inbox", type: "panel", component: "InboxPanel" };
const page = (id = "pages", component = "PagesPage"): UiContribution => ({
  location: "topbar/end",
  id,
  label: `Page ${id}`,
  type: "page",
  component,
});
const action: UiContribution = { location: "topbar/end", id: "sync", label: "Sync", type: "action", command: "demo.sync" };
const sync: UiCommand = { id: "demo.sync", title: "Sync", export: "sync" };

const link: DeepLink = { extension: "demo", location: "topbar/end", path: "x" };
const noPage = { error: "The extension Demo has no page for this link: it can't be opened." };

describe("resolveDeepLink", () => {
  it("opens the page declared after a panel at the same location", () => {
    expect(resolveDeepLink(link, [demo([panel, page()])])).toEqual({
      page: { key: "demo:pages", extension: "demo", location: "topbar/end", path: "x" },
    });
  });

  it("never opens a panel: an extension with only a panel there has no page", () => {
    expect(resolveDeepLink(link, [demo([panel])])).toEqual(noPage);
  });

  it("never opens an action: a panel and actions only, no page", () => {
    expect(resolveDeepLink(link, [demo([action, panel], { commands: [sync] })])).toEqual(noPage);
  });

  it("opens the page declared before a panel", () => {
    expect(resolveDeepLink(link, [demo([page(), panel])])).toEqual({
      page: { key: "demo:pages", extension: "demo", location: "topbar/end", path: "x" },
    });
  });

  it("opens the first page in declaration order", () => {
    const resolved = resolveDeepLink(link, [demo([page("a", "APage"), page("b", "BPage")])]);
    expect(resolved).toEqual({ page: { key: "demo:a", extension: "demo", location: "topbar/end", path: "x" } });
  });

  it("says why when the extension is absent", () => {
    expect(resolveDeepLink({ ...link, extension: "audit" }, [demo([page()])])).toEqual({
      error: "Extension “audit” is not installed: this link can't be opened.",
    });
  });

  it("says why when the extension's interface is not loaded", () => {
    expect(resolveDeepLink(link, [demo([page()], { status: "failed" })])).toEqual({
      error: "The interface of the extension Demo is not loaded: this link can't be opened.",
    });
  });

  it("says why when the location names no page location", () => {
    expect(resolveDeepLink({ ...link, location: null }, [demo([page()])])).toEqual(noPage);
  });
});
