import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Actor } from "../../shared/generated/Actor";
import type { RunListEntry } from "../../shared/generated/RunListEntry";
import type { RunState } from "../../shared/generated/RunState";
import type { WsMessage } from "../model/liveStream";
import { disposeTabCoordinator } from "./tabCoordinatorBrowser";
import { registerPageHost } from "../../shared/extensions/deepLink";
import { handleWaitingAlert, openAlertTarget, resetWaitMemory, type WaitingAlertSources } from "./useWaitingAlerts";
import { disposeWaitingAlertSettings, initWaitingAlertSettings, WAITING_ALERT_PREFS_STORAGE_KEY } from "./waitingAlertSettings";

const me: Actor = { kind: "identified", namespace: "iam", id: "louis" };

interface Shown {
  title: string;
  body?: string;
  tag?: string;
  onclick: ((e?: Event) => void) | null;
}
let shown: Shown[];
let visibility: DocumentVisibilityState;

class FakeNotification {
  static permission: NotificationPermission = "granted";
  static requestPermission = async () => FakeNotification.permission;
  onclick: ((e?: Event) => void) | null = null;
  title: string;
  constructor(title: string, options: NotificationOptions = {}) {
    this.title = title;
    shown.push(Object.assign(this, { body: options.body, tag: options.tag }));
  }
  close() {}
}

function entry(over: Partial<RunListEntry>): RunListEntry {
  return {
    run_id: "r1",
    pipeline_name: "implement-loop",
    status: "awaiting_user",
    stalled: false,
    started_at: null,
    effective_repo: "/repo",
    ...over,
  } as RunListEntry;
}

const question: WsMessage = {
  type: "event",
  event: {
    id: 12,
    run_id: "r1",
    ts: "2026-11-08T10:00:00Z",
    kind: "node_awaiting_user",
    node_id: "ask",
    iter: 1,
    payload: { cause: "declared", message: "Which storage should I use?" },
  },
};

function sources(runs: RunListEntry[], over: Partial<WaitingAlertSources> = {}): WaitingAlertSources {
  return {
    runs: () => runs,
    fetchRun: vi.fn(async (id: string) => ({ run_id: id, node_defs: [{ id: "ask", name: "implement" }] }) as unknown as RunState),
    me: () => me,
    ...over,
  };
}

function prefs(value: Record<string, unknown>) {
  localStorage.setItem(WAITING_ALERT_PREFS_STORAGE_KEY, JSON.stringify(value));
  initWaitingAlertSettings();
}

beforeEach(() => {
  shown = [];
  visibility = "hidden";
  vi.stubGlobal("Notification", FakeNotification);
  Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
  FakeNotification.permission = "granted";
  vi.spyOn(window, "focus").mockImplementation(() => {});
  resetWaitMemory();
});

afterEach(() => {
  disposeTabCoordinator();
  disposeWaitingAlertSettings();
  localStorage.clear();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  window.location.hash = "";
});

describe("handleWaitingAlert", () => {
  const mine = [entry({ name: "core-219-alerts", created_by: me })];

  it("a live wait on a hidden tab, in my Runs: one notification naming the node and the Run", async () => {
    prefs({ notification: true, sound: false, scope: "launched" });
    await handleWaitingAlert(question, sources(mine), vi.fn());
    expect(shown.map(({ title, body, tag }) => ({ title, body, tag }))).toEqual([
      {
        title: "implement is waiting for you",
        body: "core-219-alerts · Question · « Which storage should I use? »",
        tag: "r1:ask:1",
      },
    ]);
  });

  it("a child Run follows its root: launched by my Run, it alerts me", async () => {
    prefs({ notification: true, scope: "launched" });
    const runs = [
      entry({ run_id: "root", created_by: me }),
      entry({ run_id: "r1", parent_run_id: "root", root_run_id: "root", created_by: { kind: "agent", run_id: "root", node_id: "n" } }),
    ];
    await handleWaitingAlert(question, sources(runs), vi.fn());
    expect(shown).toHaveLength(1);
  });

  it("nothing while a PDO tab is in view", async () => {
    prefs({ notification: true, scope: "all" });
    visibility = "visible";
    await handleWaitingAlert(question, sources(mine), vi.fn());
    expect(shown).toEqual([]);
  });

  it("nothing for a Run out of the Portée", async () => {
    prefs({ notification: true, scope: "launched" });
    const theirs = [entry({ created_by: { kind: "identified", namespace: "iam", id: "alice" }, project_id: "p1" })];
    await handleWaitingAlert(question, sources(theirs), vi.fn());
    resetWaitMemory();
    prefs({ notification: true, scope: "projects", projects: ["p2"] });
    await handleWaitingAlert(question, sources(theirs), vi.fn());
    expect(shown).toEqual([]);
    resetWaitMemory();
    prefs({ notification: true, scope: "projects", projects: ["p1"] });
    await handleWaitingAlert(question, sources(theirs), vi.fn());
    expect(shown).toHaveLength(1);
  });

  it("nothing by default, nor on a resync", async () => {
    initWaitingAlertSettings();
    await handleWaitingAlert(question, sources(mine), vi.fn());
    resetWaitMemory();
    prefs({ notification: true, scope: "all" });
    await handleWaitingAlert({ type: "resync" }, sources(mine), vi.fn());
    expect(shown).toEqual([]);
  });

  it("a Run not listed yet is asked for its names and its root", async () => {
    prefs({ notification: true, scope: "launched" });
    const fetchRun = vi.fn(
      async () =>
        ({ run_id: "r1", name: "fresh", created_by: me, node_defs: [{ id: "ask", name: "implement" }] }) as unknown as RunState,
    );
    await handleWaitingAlert(question, sources([], { fetchRun }), vi.fn());
    expect(shown[0]).toMatchObject({ title: "implement is waiting for you", body: expect.stringContaining("fresh ·") });
    expect(fetchRun).toHaveBeenCalledTimes(1);
  });

  it("the click opens the Run on its node, once the surfaces are closed", async () => {
    prefs({ notification: true, scope: "all" });
    const open = vi.fn();
    await handleWaitingAlert(question, sources(mine), open);
    shown[0].onclick?.();
    expect(open).toHaveBeenCalledWith({ runId: "r1", nodeId: "ask" });
    const unregister = registerPageHost();
    openAlertTarget({ runId: "r1", nodeId: "ask" });
    expect(window.location.hash).toBe("#/runs/r1/nodes/ask");
    unregister();
  });

  it("a question refreshed on a wait already open alerts once; after a lift, again", async () => {
    prefs({ notification: true, scope: "all" });
    await handleWaitingAlert(question, sources(mine), vi.fn());
    const refreshed = { ...question, event: { ...question.event!, id: 13, payload: { cause: "declared", message: "Or SQLite?" } } };
    await handleWaitingAlert(refreshed, sources(mine), vi.fn());
    expect(shown).toHaveLength(1);
    await handleWaitingAlert({ ...question, event: { ...question.event!, id: 14, kind: "node_resumed", payload: null } }, sources(mine), vi.fn());
    await handleWaitingAlert({ ...refreshed, event: { ...refreshed.event, id: 15 } }, sources(mine), vi.fn());
    expect(shown).toHaveLength(2);
  });
});
