/**
 * `topbar/account` (0.8): the extensions' components at the far right of the
 * top bar, after Settings; nothing without a contribution.
 */

import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../shared/generated/ExtensionEntry";
import type { DecorationProps } from "../shared/extensions/contract";
import { hostReactMajor } from "../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../shared/extensions/store";
import TopBar from "./TopBar";

function serve(contributions: unknown[]) {
  const entry = {
    id: "acct",
    name: "Account",
    version: "1.0.0",
    ui: {
      bundle_url: "/ext/acct/ui.js",
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands: [],
      locations: [],
    },
  } as ExtensionEntry;
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify([entry]), { status: 200 })
        : new Response("not found", { status: 404 }),
    ),
  );
}

beforeEach(() => {
  resetExtensions();
  setBundleLoader(async () => ({
    Avatar: ({ context, entity }: DecorationProps) => (
      <button type="button" data-testid="avatar">
        {context.location}:{entity === null ? "no item" : "item"}
      </button>
    ),
  }));
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("the topbar/account location", () => {
  it("renders the component after Settings", async () => {
    serve([{ location: "topbar/account", id: "me", label: "Me", type: "decoration", component: "Avatar" }]);
    render(<TopBar onOpenSettings={() => {}} onOpenStats={() => {}} />);
    const avatar = await screen.findByTestId("avatar");
    expect(avatar.textContent).toBe("topbar/account:no item");
    const settings = screen.getByTestId("open-settings");
    expect(settings.compareDocumentPosition(avatar) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("renders nothing without a contribution", async () => {
    serve([]);
    render(<TopBar onOpenSettings={() => {}} onOpenStats={() => {}} />);
    await act(async () => {});
    expect(screen.queryByTestId("extension-decorations-topbar/account")).toBeNull();
  });
});
