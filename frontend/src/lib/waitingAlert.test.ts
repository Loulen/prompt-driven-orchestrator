import { describe, expect, it } from "vitest";
import type { WsMessage } from "../types";
import {
  buildNotificationSpec,
  decideWaitingAlert,
  notificationTag,
  SOUND_THROTTLE_MS,
  soundWindowOpen,
  waitingTransitionOf,
  type AlertPermission,
  type WaitingAlertInput,
  type WaitingAlertPrefs,
} from "./waitingAlert";

function event(
  kind: string,
  opts: { node?: string | null; iter?: number | null; payload?: Record<string, unknown> | null; run?: string } = {},
): WsMessage {
  return {
    type: "event",
    event: {
      id: 1,
      run_id: opts.run ?? "run-1",
      ts: "2026-11-05T10:00:00Z",
      kind,
      node_id: opts.node === undefined ? "ask" : opts.node,
      iter: opts.iter === undefined ? 2 : opts.iter,
      payload: opts.payload === undefined ? null : opts.payload,
    },
  };
}

const declaredWithMessage = event("node_awaiting_user", {
  payload: { cause: "declared", message: "Which layout do you prefer?" },
});

const BOTH_ON: WaitingAlertPrefs = { notification: true, sound: true };

function input(overrides: Partial<WaitingAlertInput> = {}): WaitingAlertInput {
  return {
    message: declaredWithMessage,
    anyTabVisible: false,
    prefs: BOTH_ON,
    permission: "granted",
    now: 1_000_000,
    lastSoundAt: null,
    ...overrides,
  };
}

describe("waitingTransitionOf — which live events are a human wait (#1011)", () => {
  it("a declared wait with a message is a question", () => {
    expect(waitingTransitionOf(declaredWithMessage)).toEqual({
      reason: "question",
      runId: "run-1",
      nodeId: "ask",
      iter: 2,
      message: "Which layout do you prefer?",
    });
  });

  it("a declared wait without a message (or a blank one) is a bare declared wait", () => {
    for (const payload of [{ cause: "declared" }, { cause: "declared", message: "   " }]) {
      expect(waitingTransitionOf(event("node_awaiting_user", { payload }))?.reason).toBe("declared");
    }
  });

  it("a legacy payload-less node_awaiting_user reads as declared, like the daemon's projection", () => {
    expect(waitingTransitionOf(event("node_awaiting_user"))?.reason).toBe("declared");
  });

  it("a refused unreleased completion is a completion wait", () => {
    const t = waitingTransitionOf(
      event("node_awaiting_user", { payload: { cause: "completion_not_released", message: "x" } }),
    );
    expect(t?.reason).toBe("completion");
    expect(t?.message).toBeNull();
  });

  it("an incident park — node_interrupted or run_interrupted — is an incident", () => {
    const node = waitingTransitionOf(
      event("node_interrupted", { payload: { reason: "session_died: tmux session vanished" } }),
    );
    expect(node).toMatchObject({ reason: "incident", nodeId: "ask", message: "session_died: tmux session vanished" });
    const run = waitingTransitionOf(
      event("run_interrupted", { node: null, iter: null, payload: { reason_code: "merge_conflict", reason: "merge conflict in node design" } }),
    );
    expect(run).toMatchObject({ reason: "incident", nodeId: null, iter: null, message: "merge conflict in node design" });
  });

  it("children_pending (by cause or by the binding marker's reason) and child_awaiting do not alert", () => {
    expect(waitingTransitionOf(event("node_awaiting_user", { payload: { cause: "children_pending" } }))).toBeNull();
    expect(waitingTransitionOf(event("node_awaiting_user", { payload: { reason: "children_pending" } }))).toBeNull();
    expect(waitingTransitionOf(event("node_awaiting_user", { payload: { cause: "child_awaiting" } }))).toBeNull();
  });

  it("any other event or message does not alert", () => {
    for (const kind of ["node_started", "node_completed", "node_failed", "node_resumed", "run_paused", "run_completed", "run_failed"]) {
      expect(waitingTransitionOf(event(kind))).toBeNull();
    }
    expect(waitingTransitionOf({ type: "resync" })).toBeNull();
    expect(waitingTransitionOf({ type: "pipeline_changed", pipeline_id: "p" })).toBeNull();
    expect(waitingTransitionOf({ type: "heartbeat" })).toBeNull();
  });

  it("a node-level event without its node is ignored", () => {
    expect(waitingTransitionOf(event("node_awaiting_user", { node: null }))).toBeNull();
    expect(waitingTransitionOf(event("node_interrupted", { node: null }))).toBeNull();
  });
});

describe("decideWaitingAlert — the matrix (#1011)", () => {
  it("alerts (notification + sound) on each human-wait transition while no tab is in view", () => {
    const messages = [
      declaredWithMessage,
      event("node_awaiting_user", { payload: { cause: "declared" } }),
      event("node_awaiting_user", { payload: { cause: "completion_not_released" } }),
      event("node_interrupted", { payload: { reason: "boom" } }),
      event("run_interrupted", { node: null, payload: { reason: "stalled" } }),
    ];
    for (const message of messages) {
      expect(decideWaitingAlert(input({ message }))).toMatchObject({ notify: true, sound: true });
    }
  });

  it("never alerts while a PDO tab is in view", () => {
    expect(decideWaitingAlert(input({ anyTabVisible: true }))).toBeNull();
  });

  it("never alerts on resync (reload, reconnection): nothing is replayed", () => {
    expect(decideWaitingAlert(input({ message: { type: "resync" } }))).toBeNull();
  });

  it("never alerts on children_pending, child_awaiting, or another event", () => {
    for (const message of [
      event("node_awaiting_user", { payload: { cause: "children_pending" } }),
      event("node_awaiting_user", { payload: { cause: "child_awaiting" } }),
      event("node_completed"),
    ]) {
      expect(decideWaitingAlert(input({ message }))).toBeNull();
    }
  });

  it("both switches off (the default) → nothing", () => {
    expect(decideWaitingAlert(input({ prefs: { notification: false, sound: false } }))).toBeNull();
  });

  it("each switch is independent", () => {
    expect(decideWaitingAlert(input({ prefs: { notification: true, sound: false } }))).toMatchObject({
      notify: true,
      sound: false,
    });
    expect(decideWaitingAlert(input({ prefs: { notification: false, sound: true } }))).toMatchObject({
      notify: false,
      sound: true,
    });
  });

  it("without the granted permission only the sound is possible", () => {
    const blocked: AlertPermission[] = ["default", "denied", "insecure", "unavailable"];
    for (const permission of blocked) {
      expect(decideWaitingAlert(input({ permission }))).toMatchObject({ notify: false, sound: true });
      expect(
        decideWaitingAlert(input({ permission, prefs: { notification: true, sound: false } })),
      ).toBeNull();
    }
  });

  it("a burst: one notification per transition, one sound per 10 s window", () => {
    const now = 5_000_000;
    // 3 s after the last sound: notified, no sound.
    expect(decideWaitingAlert(input({ now, lastSoundAt: now - 3_000 }))).toMatchObject({
      notify: true,
      sound: false,
    });
    // Just under the window: still no sound.
    expect(
      decideWaitingAlert(input({ now, lastSoundAt: now - SOUND_THROTTLE_MS + 1 }))?.sound,
    ).toBe(false);
    // The window has elapsed: sound again.
    expect(decideWaitingAlert(input({ now, lastSoundAt: now - SOUND_THROTTLE_MS }))?.sound).toBe(true);
    expect(SOUND_THROTTLE_MS).toBe(10_000);
  });

  it("sound-only inside the window decides nothing at all", () => {
    expect(
      decideWaitingAlert(
        input({ prefs: { notification: false, sound: true }, now: 100_000, lastSoundAt: 95_000 }),
      ),
    ).toBeNull();
  });
});

describe("decideWaitingAlert — the sound (#1012)", () => {
  it("each reason has its own chime", () => {
    const cases: Array<[WsMessage, string]> = [
      [declaredWithMessage, "celesta"],
      [event("node_awaiting_user", { payload: { cause: "declared" } }), "softGong"],
      [event("node_awaiting_user", { payload: { cause: "completion_not_released" } }), "kalimbaTriad"],
      [event("node_interrupted", { payload: { reason: "boom" } }), "glassPing"],
      [event("run_interrupted", { node: null, payload: { reason: "stalled" } }), "glassPing"],
    ];
    for (const [message, chime] of cases) {
      expect(decideWaitingAlert(input({ message }))?.chime).toBe(chime);
    }
  });

  it("no chime when the sound is off or its window is closed", () => {
    expect(decideWaitingAlert(input({ prefs: { notification: true, sound: false } }))?.chime).toBeNull();
    expect(decideWaitingAlert(input({ now: 100_000, lastSoundAt: 95_000 }))?.chime).toBeNull();
  });

  it("a sound alone works with the notification refused or impossible", () => {
    for (const permission of ["denied", "insecure", "unavailable"] as AlertPermission[]) {
      expect(
        decideWaitingAlert(input({ permission, prefs: { notification: false, sound: true } })),
      ).toMatchObject({ notify: false, sound: true, chime: "celesta" });
    }
  });

  it("soundWindowOpen: never played, or 10 s ago or more", () => {
    expect(soundWindowOpen(null, 0)).toBe(true);
    expect(soundWindowOpen(0, 9_999)).toBe(false);
    expect(soundWindowOpen(0, 10_000)).toBe(true);
  });
});

describe("buildNotificationSpec — the notification's contract (#1011)", () => {
  const t = (message: WsMessage) => waitingTransitionOf(message)!;

  it("titles with the Run's name, falls back on its id", () => {
    expect(buildNotificationSpec(t(declaredWithMessage), { runName: "feature-login" }).title).toBe(
      "PDO — feature-login is waiting for you",
    );
    expect(buildNotificationSpec(t(declaredWithMessage)).title).toBe("PDO — run-1 is waiting for you");
    expect(buildNotificationSpec(t(declaredWithMessage), { runName: "  " }).title).toBe(
      "PDO — run-1 is waiting for you",
    );
  });

  it("the body is the agent's message as is", () => {
    expect(buildNotificationSpec(t(declaredWithMessage), { nodeName: "Ask" }).body).toBe(
      "Which layout do you prefer?",
    );
  });

  it("a declared wait without a message names the node (name, else id)", () => {
    const bare = t(event("node_awaiting_user", { payload: { cause: "declared" } }));
    expect(buildNotificationSpec(bare, { nodeName: "Designer" }).body).toBe("Node Designer is waiting for you");
    expect(buildNotificationSpec(bare).body).toBe("Node ask is waiting for you");
  });

  it("a completion wait says the node is done and waits for a go", () => {
    const done = t(event("node_awaiting_user", { payload: { cause: "completion_not_released" } }));
    expect(buildNotificationSpec(done, { nodeName: "Implement" }).body).toBe(
      "Node Implement is done and waits for your go",
    );
  });

  it("an incident park gives the incident's reason", () => {
    const incident = t(event("run_interrupted", { node: null, payload: { reason: "merge conflict in node design" } }));
    expect(buildNotificationSpec(incident).body).toBe("Run paused on an incident: merge conflict in node design");
    const noReason = t(event("node_interrupted", { payload: {} }));
    expect(buildNotificationSpec(noReason).body).toBe("Run paused on an incident");
  });

  it("tags run:node:iter, or just the run for a run-level park", () => {
    expect(buildNotificationSpec(t(declaredWithMessage)).tag).toBe("run-1:ask:2");
    const runPark = t(event("run_interrupted", { node: null, iter: null, payload: { reason: "x" } }));
    expect(buildNotificationSpec(runPark).tag).toBe("run-1");
    expect(notificationTag({ runId: "r", nodeId: "n", iter: null })).toBe("r:n:1");
  });

  it("targets the Run and its waiting node", () => {
    expect(buildNotificationSpec(t(declaredWithMessage)).target).toEqual({ runId: "run-1", nodeId: "ask" });
    const runPark = t(event("run_interrupted", { node: null, payload: {} }));
    expect(buildNotificationSpec(runPark).target).toEqual({ runId: "run-1", nodeId: null });
  });
});
