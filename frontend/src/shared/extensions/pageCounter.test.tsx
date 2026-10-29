/**
 * CORE-125 — the counter of a Page of `topbar/end` (API 0.19): read for the
 * current Actor from the path the page declares, shown on its icon (neutral,
 * or in the accent), read again at each event of its extension and at each
 * resync; nothing for a page that declares none.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WsMessage } from "../../run/model/liveStream";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiContribution } from "../generated/UiContribution";
import ExtensionContextProvider from "./ExtensionContextProvider";
import ExtensionTopbarEnd from "./ExtensionTopbarEnd";
import { COUNTER_SETTLE_MS, isEventOf } from "./pageCounter";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader } from "./store";

const listeners = vi.hoisted(() => [] as ((msg: WsMessage) => void)[]);
vi.mock("../../run/store/useDaemonSocket", () => ({
  subscribe: (listener: (msg: WsMessage) => void) => {
    listeners.push(listener);
    return () => listeners.splice(listeners.indexOf(listener), 1);
  },
}));

function emit(msg: WsMessage) {
  act(() => listeners.forEach((listener) => listener(msg)));
}

function extension(id: string, contributions: UiContribution[]): ExtensionEntry {
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
  };
}

function page(id: string, counter?: string): UiContribution {
  return { location: "topbar/end", id, label: "Demo", type: "page", component: "Page", ...(counter ? { counter } : {}) };
}

let answers: Record<string, unknown>[] = [];
let counterReads = 0;

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      if (url === "/ext") return Response.json(extensions);
      if (url === "/ext/demo/counter") {
        counterReads += 1;
        return Response.json(answers[Math.min(counterReads, answers.length) - 1]);
      }
      return new Response("not found", { status: 404 });
    }),
  );
}

function shell() {
  return render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <ExtensionTopbarEnd />
    </ExtensionContextProvider>,
  );
}

beforeEach(() => {
  resetExtensions();
  setBundleLoader(async () => ({ Page: () => null }));
  listeners.length = 0;
  counterReads = 0;
  answers = [];
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.useRealTimers();
  resetExtensions();
});

describe("the counter of a page of topbar/end", () => {
  it("shows the count it reads for the Actor, neutral, its detail in the tooltip", async () => {
    answers = [{ count: 3, accent: false, detail: "3 sessions you started" }];
    serve([extension("demo", [page("home", "/ext/demo/counter")])]);
    shell();
    const count = await screen.findByTestId("extension-topbar-end-count-demo:home");
    expect(count).toHaveTextContent("3");
    expect(count).not.toHaveAttribute("data-accent");
    expect(screen.getByTestId("extension-topbar-end-demo:home")).toHaveAccessibleName("Demo, 3");
  });

  it("turns to the accent and follows its extension's events, and only those", async () => {
    answers = [
      { count: 1, accent: false },
      { count: 2, accent: true, detail: "1 waiting" },
      { count: 0, accent: false },
    ];
    serve([extension("demo", [page("home", "/ext/demo/counter")])]);
    shell();
    const count = await screen.findByTestId("extension-topbar-end-count-demo:home");
    expect(count).toHaveTextContent("1");

    // Another extension's event: no read.
    emit({ type: "hosted_session_changed", session: { id: "pdo-hosted-x", extension: "other" } });
    await new Promise((resolve) => setTimeout(resolve, COUNTER_SETTLE_MS * 2));
    expect(counterReads).toBe(1);

    emit({ type: "hosted_session_changed", session: { id: "pdo-hosted-a", extension: "demo", state: "awaiting_user" } });
    await waitFor(() => expect(screen.getByTestId("extension-topbar-end-count-demo:home")).toHaveTextContent("2"));
    expect(screen.getByTestId("extension-topbar-end-count-demo:home")).toHaveAttribute("data-accent", "true");

    // Nothing left: no pill at all.
    emit({ type: "resync" });
    await waitFor(() => expect(screen.queryByTestId("extension-topbar-end-count-demo:home")).toBeNull());
    expect(counterReads).toBe(3);
  });

  it("reads once for a burst of events", async () => {
    answers = [{ count: 1 }, { count: 4 }];
    serve([extension("demo", [page("home", "/ext/demo/counter")])]);
    shell();
    await screen.findByTestId("extension-topbar-end-count-demo:home");
    emit({ type: "extension_event", extension: "demo" });
    emit({ type: "hosted_live_artifacts_changed", extension: "demo", id: "pdo-hosted-a", artifacts: [] });
    emit({ type: "hosted_session_forgotten", session: { id: "pdo-hosted-a", extension: "demo" } });
    await waitFor(() => expect(screen.getByTestId("extension-topbar-end-count-demo:home")).toHaveTextContent("4"));
    expect(counterReads).toBe(2);
  });

  it("changes nothing for a page that declares none, nor when its read fails", async () => {
    serve([extension("demo", [page("home"), page("broken", "/ext/demo/missing")])]);
    shell();
    const button = await screen.findByTestId("extension-topbar-end-demo:home");
    await screen.findByTestId("extension-topbar-end-demo:broken");
    await new Promise((resolve) => setTimeout(resolve, 20));
    expect(screen.queryByTestId("extension-topbar-end-count-demo:home")).toBeNull();
    expect(screen.queryByTestId("extension-topbar-end-count-demo:broken")).toBeNull();
    expect(button).toHaveAccessibleName("Demo");
    expect(counterReads).toBe(0);
  });
});

describe("the events of an extension", () => {
  it("are what it published and what is about its Sessions hébergées", () => {
    expect(isEventOf({ type: "extension_event", extension: "demo" }, "demo")).toBe(true);
    expect(isEventOf({ type: "hosted_session_changed", session: { id: "s", extension: "demo" } }, "demo")).toBe(true);
    expect(isEventOf({ type: "hosted_session_changed", session: { id: "s", extension: "b" } }, "demo")).toBe(false);
    expect(isEventOf({ type: "hosted_live_artifacts_changed", extension: "demo" }, "demo")).toBe(true);
    expect(isEventOf({ type: "pipeline_changed" }, "demo")).toBe(false);
  });
});
