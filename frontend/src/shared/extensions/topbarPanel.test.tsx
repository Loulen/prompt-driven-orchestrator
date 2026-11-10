/**
 * S3 — a panel of `topbar/end` (API 0.20): its icon carries a counter (absent
 * at 0, bounded at 99+) and opens the panel under it; Escape, a click outside
 * and the icon close it, a click in a toast does not, so « Undo » works inside
 * it. An action's icon may carry a counter too.
 */
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiCommand } from "../generated/UiCommand";
import type { UiContribution } from "../generated/UiContribution";
import { toast } from "../ui/kit";
import { resetToasts } from "../ui/toast";
import Toaster from "../ui/Toaster";
import type { PanelProps } from "./contract";
import ExtensionContextProvider from "./ExtensionContextProvider";
import ExtensionTopbarEnd from "./ExtensionTopbarEnd";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader } from "./store";

vi.mock("../../run/store/useDaemonSocket", () => ({ subscribe: () => () => {} }));

function extension(contributions: UiContribution[], commands: UiCommand[] = []): ExtensionEntry {
  return {
    id: "demo",
    name: "Demo",
    version: "1.0.0",
    ui: {
      bundle_url: "/ext/demo/ui.js",
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands,
      locations: [],
    },
  };
}

const panel = (counter?: string): UiContribution => ({
  location: "topbar/end",
  id: "inbox",
  label: "Inbox",
  type: "panel",
  component: "InboxPanel",
  ...(counter ? { counter } : {}),
});

/** The panel of the bundle: three items, each deleted with an « Undo ». */
function InboxPanel({ close, context }: PanelProps) {
  const [items, setItems] = useState(["One", "Two", "Three"]);
  return (
    <div>
      <p>{`at ${context.location}`}</p>
      {items.map((item) => (
        <button
          key={item}
          type="button"
          onClick={() => {
            setItems((all) => all.filter((i) => i !== item));
            toast(`${item} deleted`, "info", { actionLabel: "Undo", onAction: () => setItems((all) => [...all, item]) });
          }}
        >
          {`Delete ${item}`}
        </button>
      ))}
      <button type="button" onClick={close}>
        Follow
      </button>
    </div>
  );
}

let count = 3;

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url === "/ext") return Response.json(extensions);
      if (url === "/ext/demo/counter") return Response.json({ count, accent: false });
      return new Response("not found", { status: 404 });
    }),
  );
}

function shell() {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <button type="button">Elsewhere</button>
      <ExtensionTopbarEnd />
      <Toaster />
    </ExtensionContextProvider>,
  );
}

const commandCalls: unknown[] = [];

beforeEach(() => {
  resetExtensions();
  resetToasts();
  count = 3;
  commandCalls.length = 0;
  setBundleLoader(async () => ({ InboxPanel, sync: (context: unknown) => void commandCalls.push(context) }));
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("a panel of topbar/end", () => {
  it("opens under its icon, which carries its counter, and gets the location's context", async () => {
    const user = userEvent.setup();
    serve([extension([panel("/ext/demo/counter")])]);
    shell();
    const icon = await screen.findByTestId("extension-topbar-end-demo:inbox");
    expect(await screen.findByTestId("extension-topbar-end-count-demo:inbox")).toHaveTextContent("3");
    expect(icon).toHaveAccessibleName("Inbox, 3");
    expect(icon).toHaveAttribute("aria-expanded", "false");
    await user.click(icon);
    const dialog = await screen.findByRole("dialog", { name: "Inbox" });
    expect(within(dialog).getByText("at topbar/end")).toBeInTheDocument();
    expect(icon).toHaveAttribute("aria-expanded", "true");
    // The icon toggles it.
    await user.click(icon);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("stays open on « Undo » in the toast; Escape and a click outside close it", async () => {
    const user = userEvent.setup();
    serve([extension([panel()])]);
    shell();
    await user.click(await screen.findByTestId("extension-topbar-end-demo:inbox"));
    const dialog = await screen.findByRole("dialog", { name: "Inbox" });
    await user.click(within(dialog).getByRole("button", { name: "Delete Two" }));
    expect(within(dialog).queryByRole("button", { name: "Delete Two" })).toBeNull();
    await user.click(within(await screen.findByTestId("toast-info")).getByRole("button", { name: "Undo" }));
    expect(screen.getByRole("dialog", { name: "Inbox" })).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "Delete Two" })).toBeInTheDocument();

    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(screen.getByTestId("extension-topbar-end-demo:inbox")).toHaveFocus();

    await user.click(screen.getByTestId("extension-topbar-end-demo:inbox"));
    await screen.findByRole("dialog", { name: "Inbox" });
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("closes itself through `close`", async () => {
    const user = userEvent.setup();
    serve([extension([panel()])]);
    shell();
    await user.click(await screen.findByTestId("extension-topbar-end-demo:inbox"));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Follow" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("shows no counter at 0, and 99+ past 99", async () => {
    count = 0;
    serve([extension([panel("/ext/demo/counter")])]);
    const { unmount } = shell();
    await screen.findByTestId("extension-topbar-end-demo:inbox");
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByTestId("extension-topbar-end-count-demo:inbox")).toBeNull();
    unmount();

    count = 120;
    shell();
    expect(await screen.findByTestId("extension-topbar-end-count-demo:inbox")).toHaveTextContent("99+");
    expect(screen.getByTestId("extension-topbar-end-demo:inbox")).toHaveAccessibleName("Inbox, 120");
  });
});

describe("an action of topbar/end", () => {
  it("may carry a counter on its icon, and still calls its command", async () => {
    const user = userEvent.setup();
    serve([
      extension(
        [{ location: "topbar/end", id: "sync", label: "Sync", type: "action", command: "demo.sync", counter: "/ext/demo/counter" }],
        [{ id: "demo.sync", title: "Sync", export: "sync" }],
      ),
    ]);
    shell();
    expect(await screen.findByTestId("extension-topbar-end-count-demo:sync")).toHaveTextContent("3");
    await act(() => user.click(screen.getByTestId("extension-topbar-end-demo:sync")));
    await waitFor(() => expect(commandCalls).toHaveLength(1));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
