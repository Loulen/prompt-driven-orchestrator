/**
 * The Workbench's top bar: the command field and its palette, the core's
 * icons each named in a tooltip, the extensions' `topbar/end` icons before
 * them, and the theme.
 */

import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../shared/generated/ExtensionEntry";
import { hostReactMajor } from "../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../shared/extensions/store";
import { resetPages } from "../shared/extensions/deepLink";
import { setMode, setTheme } from "../hooks/useTheme";
import { runCreationRights, UNKNOWN_RIGHTS } from "../run/model/runCreationRights";
import { useRunCreationRightsStore } from "../run/store/runCreationRightsStore";
import type { Project } from "../workspace/model/project";
import { usePalette } from "./palette/paletteStore";
import TopBar from "./TopBar";

/** An extension with one `topbar/end` page. */
function withPage(id: string, label: string): ExtensionEntry {
  return {
    id,
    name: label,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions: [{ location: "topbar/end", id: "home", label, type: "page", component: "Home" }],
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

function renderBar(onNewRun = vi.fn()) {
  const handlers = { onOpenSettings: vi.fn(), onOpenStats: vi.fn(), onNewRun };
  render(<TopBar {...handlers} />);
  return handlers;
}

/** The tooltip a hover on `el` shows. */
async function tooltipOf(el: HTMLElement): Promise<string> {
  fireEvent.pointerEnter(el);
  const tip = await screen.findByTestId("tooltip-content");
  const text = tip.textContent ?? "";
  fireEvent.pointerLeave(el);
  await waitFor(() => expect(screen.queryByTestId("tooltip-content")).toBeNull());
  return text;
}

beforeEach(() => {
  resetExtensions();
  resetPages();
  usePalette.getState().hide();
  useRunCreationRightsStore.setState({ rights: UNKNOWN_RIGHTS, projects: [] });
  setBundleLoader(async () => ({ Home: () => <p>home</p> }));
  serve([]);
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
  setMode("dark");
});

describe("the top bar", () => {
  it("names each of its icons in a tooltip", async () => {
    renderBar();
    expect(await tooltipOf(screen.getByTestId("topbar-new-run"))).toBe("New run");
    expect(await tooltipOf(screen.getByTestId("open-stats"))).toBe("Stats");
    expect(await tooltipOf(screen.getByTestId("open-settings"))).toBe("Settings");
    expect(await tooltipOf(screen.getByTestId("dark-mode-toggle"))).toBe("Dark mode off");
  });

  it("opens Stats, Settings and New run", () => {
    const handlers = renderBar();
    fireEvent.click(screen.getByTestId("open-stats"));
    fireEvent.click(screen.getByTestId("open-settings"));
    fireEvent.click(screen.getByTestId("topbar-new-run"));
    expect(handlers.onOpenStats).toHaveBeenCalledTimes(1);
    expect(handlers.onOpenSettings).toHaveBeenCalledTimes(1);
    expect(handlers.onNewRun).toHaveBeenCalledTimes(1);
  });

  it("greys New run, with its reason, where no Projet allows a Run", async () => {
    const P1 = { id: "p1", name: "One", members: ["/r"] } as unknown as Project;
    useRunCreationRightsStore.setState({
      rights: runCreationRights([P1], [
        { decision: "deny", reason: "x" },
        { decision: "deny", reason: "x" },
        { decision: "deny", reason: "x" },
      ]),
    });
    const handlers = renderBar();
    const button = screen.getByTestId("topbar-new-run");
    expect(button).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(button);
    expect(handlers.onNewRun).not.toHaveBeenCalled();
    expect(await tooltipOf(button)).toContain("can't create Runs");
  });

  it("switches the Dark mode only, the Theme untouched", () => {
    setTheme("classic");
    setMode("dark");
    renderBar();
    fireEvent.click(screen.getByTestId("dark-mode-toggle"));
    expect(document.documentElement.getAttribute("data-mode")).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("classic");
    expect(screen.getByTestId("dark-mode-toggle")).toHaveAccessibleName("Dark mode on");
    setTheme("acogni");
  });

  it("puts the extensions' topbar/end icons before the core's, one tooltip each", async () => {
    serve([withPage("one", "Docs"), withPage("two", "Reports"), withPage("three", "Deployments")]);
    renderBar();
    const group = await screen.findByRole("group", { name: "Extensions" });
    await waitFor(() => expect(within(group).getAllByRole("button")).toHaveLength(3));
    const names = within(group).getAllByRole("button").map((b) => b.getAttribute("aria-label"));
    expect(names).toEqual(["Docs", "Reports", "Deployments"]);
    // Before New run: the group comes first in the right-hand cluster.
    expect(group.compareDocumentPosition(screen.getByTestId("topbar-new-run"))).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING,
    );
    expect(await tooltipOf(within(group).getAllByRole("button")[1])).toBe("Reports");
  });
});

describe("the command field", () => {
  it("opens the palette on a click, says when nothing matches, and Escape closes it", async () => {
    renderBar();
    expect(screen.queryByTestId("command-palette")).toBeNull();
    fireEvent.click(screen.getByTestId("command-field"));
    const input = screen.getByTestId("command-palette-input");
    await waitFor(() => expect(input).toHaveFocus());
    fireEvent.change(input, { target: { value: "deploy" } });
    expect(screen.getByTestId("command-palette-empty")).toHaveTextContent("No match");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(screen.queryByTestId("command-palette")).toBeNull();
    // Closed, the query is gone.
    expect(usePalette.getState().query).toBe("");
  });

  it("opens and closes with Ctrl+K, and closes on a click outside", () => {
    renderBar();
    act(() => {
      fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    });
    expect(screen.getByTestId("command-palette")).toBeInTheDocument();
    act(() => {
      fireEvent.keyDown(window, { key: "K", metaKey: true });
    });
    expect(screen.queryByTestId("command-palette")).toBeNull();
    fireEvent.click(screen.getByTestId("command-field"));
    fireEvent.mouseDown(screen.getByTestId("command-palette-scrim"));
    expect(screen.queryByTestId("command-palette")).toBeNull();
  });
});
