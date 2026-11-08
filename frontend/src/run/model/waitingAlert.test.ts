import { describe, expect, it } from "vitest";
import type { EventKind } from "../../shared/generated/EventKind";
import type { WsMessage } from "./liveStream";
import { chimeMelody } from "./chimes";
import {
  buildNotificationSpec,
  createWaitMemory,
  decideWaitingAlert,
  incidentProse,
  notificationTag,
  SOUND_THROTTLE_MS,
  soundWindowOpen,
  waitingTransitionOf,
  type WaitingAlertInput,
} from "./waitingAlert";

function event(kind: EventKind, payload: Record<string, unknown> | null, nodeId: string | null = "ask", iter: number | null = 2): WsMessage {
  return { type: "event", event: { id: 7, run_id: "r1", ts: "2026-11-08T10:00:00Z", kind, node_id: nodeId, iter, payload } };
}

const question = event("node_awaiting_user", { cause: "declared", message: "Which storage should I use?" });

function input(over: Partial<WaitingAlertInput> = {}): WaitingAlertInput {
  return {
    message: question,
    anyTabVisible: false,
    prefs: { notification: true, sound: true },
    permission: "granted",
    now: 1_000_000,
    lastSoundAt: null,
    ...over,
  };
}

describe("waitingTransitionOf: a live transition to a human wait", () => {
  it("reads a declared wait with a message as a question, without one as a declared wait", () => {
    expect(waitingTransitionOf(question)).toEqual({
      reason: "question",
      runId: "r1",
      nodeId: "ask",
      iter: 2,
      message: "Which storage should I use?",
    });
    expect(waitingTransitionOf(event("node_awaiting_user", { cause: "declared" }))?.reason).toBe("declared");
    // A payload-less event reads as declared, as the daemon's projection does.
    expect(waitingTransitionOf(event("node_awaiting_user", null))?.reason).toBe("declared");
  });

  it("reads a completion not released, and an incident park of a node or of the Run", () => {
    expect(waitingTransitionOf(event("node_awaiting_user", { cause: "completion_not_released" }))?.reason).toBe("completion");
    expect(waitingTransitionOf(event("node_interrupted", { reason: "session_died: the session crashed" }))).toMatchObject({
      reason: "incident",
      nodeId: "ask",
      message: "the session crashed",
    });
    expect(
      waitingTransitionOf(event("run_interrupted", { reason_code: "run_stalled", reason: "run_stalled: nothing moves" }, null, null)),
    ).toMatchObject({ reason: "incident", nodeId: null, message: "nothing moves" });
  });

  it("never alerts on children_pending, a derived child wait, a budget reached, a resync or another event", () => {
    expect(waitingTransitionOf(event("node_awaiting_user", { reason: "children_pending" }))).toBeNull();
    expect(waitingTransitionOf(event("node_awaiting_user", { cause: "child_awaiting" }))).toBeNull();
    expect(
      waitingTransitionOf(event("run_interrupted", { reason_code: "region_budget_exhausted", reason: "budget" }, null, null)),
    ).toBeNull();
    expect(waitingTransitionOf({ type: "resync" })).toBeNull();
    expect(waitingTransitionOf(event("node_started", null))).toBeNull();
    expect(waitingTransitionOf(event("node_awaiting_user", { cause: "declared" }, null))).toBeNull();
  });
});

describe("decideWaitingAlert", () => {
  it("alerts with both, the chime of the reason", () => {
    expect(decideWaitingAlert(input())).toMatchObject({ notify: true, sound: true, chime: "rising" });
  });

  it("stays silent while a PDO tab of this browser is in view", () => {
    expect(decideWaitingAlert(input({ anyTabVisible: true }))).toBeNull();
  });

  it("does nothing with both switches off (the default)", () => {
    expect(decideWaitingAlert(input({ prefs: { notification: false, sound: false } }))).toBeNull();
  });

  it("notifies only with the permission granted; the sound needs none", () => {
    expect(decideWaitingAlert(input({ permission: "insecure" }))).toMatchObject({ notify: false, sound: true });
    expect(decideWaitingAlert(input({ permission: "denied", prefs: { notification: true, sound: false } }))).toBeNull();
  });

  it("rings at most once per window", () => {
    const now = 1_000_000;
    expect(soundWindowOpen(null, now)).toBe(true);
    expect(soundWindowOpen(now - SOUND_THROTTLE_MS + 1, now)).toBe(false);
    expect(soundWindowOpen(now - SOUND_THROTTLE_MS, now)).toBe(true);
    expect(decideWaitingAlert(input({ lastSoundAt: now - 1_000 }))).toMatchObject({ notify: true, sound: false, chime: null });
  });
});

describe("the notification", () => {
  it("is tagged run:node:iter, the Run alone for a park of the Run", () => {
    expect(notificationTag({ runId: "r1", nodeId: "ask", iter: 2 })).toBe("r1:ask:2");
    expect(notificationTag({ runId: "r1", nodeId: "ask", iter: null })).toBe("r1:ask:1");
    expect(notificationTag({ runId: "r1", nodeId: null, iter: null })).toBe("r1");
  });

  it("names the node and the Run, one text per reason", () => {
    const labels = { runName: "core-219-alerts", nodeName: "implement" };
    expect(buildNotificationSpec(waitingTransitionOf(question)!, labels)).toEqual({
      title: "implement is waiting for you",
      body: "core-219-alerts · Question · « Which storage should I use? »",
      tag: "r1:ask:2",
      target: { runId: "r1", nodeId: "ask" },
    });
    const completion = waitingTransitionOf(event("node_awaiting_user", { cause: "completion_not_released" }))!;
    expect(buildNotificationSpec(completion, labels)).toMatchObject({
      title: "implement is done, waiting for your go",
      body: "core-219-alerts · Approve to complete it",
    });
    const incident = waitingTransitionOf(event("node_interrupted", { reason: "session_died: the session crashed" }))!;
    expect(buildNotificationSpec(incident, { runName: "nightly-triage", nodeName: "deploy" })).toMatchObject({
      title: "Run paused on an incident",
      body: "nightly-triage · deploy · the session crashed",
    });
  });

  it("falls back on the ids when the names are unknown", () => {
    expect(buildNotificationSpec(waitingTransitionOf(event("node_awaiting_user", { cause: "declared" }))!)).toMatchObject({
      title: "ask is waiting for you",
      body: "r1 · Waiting for your answer",
    });
  });
});

describe("chimes and incident prose", () => {
  it("a question rises, a completion is one note, an incident falls low", () => {
    const [a, b] = chimeMelody("rising");
    expect(b).toBeGreaterThan(a);
    expect(chimeMelody("soft")).toHaveLength(1);
    const [c, d] = chimeMelody("falling");
    expect(d).toBeLessThan(c);
    expect(c).toBeLessThan(a);
  });

  it("keeps an un-prefixed reason whole", () => {
    expect(incidentProse("the merge failed")).toBe("the merge failed");
    expect(incidentProse(null)).toBeNull();
  });
});

describe("createWaitMemory: a refresh is no transition", () => {
  it("a second question on the same open wait refreshes it; a lift or a resync reopens", () => {
    const waits = createWaitMemory();
    expect(waits.refreshes(question)).toBe(false);
    expect(waits.refreshes(event("node_awaiting_user", { cause: "declared", message: "Or SQLite?" }))).toBe(true);
    // Another iteration is another wait.
    expect(waits.refreshes(event("node_awaiting_user", { cause: "declared" }, "ask", 3))).toBe(false);
    expect(waits.refreshes(event("node_resumed", null))).toBe(false);
    expect(waits.refreshes(question)).toBe(false);
    expect(waits.refreshes({ type: "resync" })).toBe(false);
    expect(waits.refreshes(question)).toBe(false);
  });

  it("ignores what is no human wait", () => {
    const waits = createWaitMemory();
    const pending = event("node_awaiting_user", { reason: "children_pending" });
    expect(waits.refreshes(pending)).toBe(false);
    expect(waits.refreshes(pending)).toBe(false);
  });
});

describe("chimeVoices", () => {
  it("one sine per note and partial, rising then decaying, the peaks sharing the note's gain", async () => {
    const { CHIMES, chimeVoices } = await import("./chimes");
    const voices = chimeVoices(CHIMES.rising);
    expect(voices).toHaveLength(CHIMES.rising.notes.length * CHIMES.rising.partials.length);
    for (const v of voices) expect(v.end).toBeGreaterThan(v.peakAt);
    const firstNote = voices.slice(0, CHIMES.rising.partials.length);
    expect(firstNote.reduce((sum, v) => sum + v.peak, 0)).toBeCloseTo(CHIMES.rising.notes[0].gain);
  });
});
