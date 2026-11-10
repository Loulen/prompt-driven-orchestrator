/**
 * CORE-42: the gestures on a Run and its nodes, as `POST /authorize` answers
 * them. The questions are the Operations the daemon asks before each
 * gesture; a refusal hides one gesture of one Run (or node), nothing else.
 */
import { describe, expect, it } from "vitest";
import type { Decision } from "../../shared/authorization/api";
import {
  batches,
  gestureQuestions,
  mayOnNode,
  mayOnRun,
  NODE_GESTURES,
  whyNotOnNode,
  whyNotOnRun,
  NOTHING_REFUSED,
  RUN_ROW_GESTURES,
  withDecisions,
  type GestureQuestion,
} from "./runGestures";

const allow: Decision = { decision: "allow" };
const SENTENCE = "You can't change Runs in Project P. Ask an administrator of P.";
const deny: Decision = { decision: "deny", reason: "the test rules refuse it", sentence: SENTENCE };

describe("the questions of a Run's gestures", () => {
  it("asks each gesture on the Run, then each gesture on each node, as the daemon asks them", () => {
    const questions = gestureQuestions([
      { runId: "r1", runGestures: ["pause_run", "delete"], nodeIds: ["worker"] },
    ]);
    expect(questions.slice(0, 3)).toEqual([
      { resource: "run", verb: { action: "pause_run" }, target: "r1" },
      { resource: "run", verb: "delete", target: "r1" },
      { resource: "node", verb: { action: "stop" }, target: "r1/worker" },
    ]);
    expect(questions).toHaveLength(2 + NODE_GESTURES.length);
    // No Projet: the daemon places them in the Projet of the Run.
    expect(questions.every((q) => q.projects === undefined)).toBe(true);
  });
});

describe("the rights the decisions grant", () => {
  const question: GestureQuestion = { runId: "r1", runGestures: ["pause_run", "retry_all"], nodeIds: ["a", "b"] };

  it("refuses only what was denied, where it was denied", () => {
    const decisions = gestureQuestions([question]).map((q) =>
      q.target === "r1/a" && (q.verb as { action: string }).action === "stop" ? deny : q.target === "r1" && (q.verb as { action: string }).action === "retry_all" ? deny : allow,
    );
    const rights = withDecisions(NOTHING_REFUSED, [question], decisions);
    expect(mayOnRun(rights, "r1", "pause_run")).toBe(true);
    expect(mayOnRun(rights, "r1", "retry_all")).toBe(false);
    expect(mayOnNode(rights, "r1", "a", "stop")).toBe(false);
    expect(mayOnNode(rights, "r1", "b", "stop")).toBe(true);
    expect(mayOnNode(rights, "r1", "a", "retry")).toBe(true);
    // Another Run, never asked about: nothing hidden.
    expect(mayOnRun(rights, "r2", "retry_all")).toBe(true);
    // A refusal keeps the sentence a person reads, never the technical reason.
    expect(whyNotOnRun(rights, "r1", "retry_all")).toBe(SENTENCE);
    expect(whyNotOnNode(rights, "r1", "a", "stop")).toBe(SENTENCE);
    expect(whyNotOnRun(rights, "r1", "pause_run")).toBeUndefined();
  });

  it("says the core's generic sentence when the extension gives none (CORE-205)", () => {
    const row: GestureQuestion = { runId: "r1", runGestures: ["retry_all"], nodeIds: [] };
    const rights = withDecisions(NOTHING_REFUSED, [row], [{ decision: "deny", reason: "the test rules refuse it" }]);
    expect(whyNotOnRun(rights, "r1", "retry_all")).toBe("You don't have the rights to do this.");
  });

  it("takes the new answer of what is asked again, and keeps the rest", () => {
    const allDenied = gestureQuestions([question]).map(() => deny);
    const refused = withDecisions(NOTHING_REFUSED, [question], allDenied);
    const row: GestureQuestion = { runId: "r1", runGestures: ["retry_all"], nodeIds: [] };
    const again = withDecisions(refused, [row], [allow]);
    expect(mayOnRun(again, "r1", "retry_all")).toBe(true);
    expect(mayOnRun(again, "r1", "pause_run")).toBe(false);
    expect(mayOnNode(again, "r1", "a", "stop")).toBe(false);
  });

  it("hides nothing without an extension (every answer allows) or without an answer", () => {
    const rights = withDecisions(NOTHING_REFUSED, [question], gestureQuestions([question]).map(() => allow));
    expect(rights.refused.size).toBe(0);
    expect(mayOnNode(NOTHING_REFUSED, "r1", "a", "mark_node_done")).toBe(true);
  });
});

describe("the batches of a question", () => {
  it("never holds more Operations than the daemon accepts, in the order asked", () => {
    const rows = Array.from({ length: 100 }, (_, i) => ({ runId: `r${i}`, runGestures: RUN_ROW_GESTURES, nodeIds: [] }));
    const big: GestureQuestion = { runId: "big", runGestures: ["pause_run"], nodeIds: Array.from({ length: 80 }, (_, i) => `n${i}`) };
    const parts = batches([...rows, big], 500);
    for (const part of parts) expect(gestureQuestions(part).length).toBeLessThanOrEqual(500);
    expect(gestureQuestions(parts.flat())).toEqual(gestureQuestions([...rows, big]));
  });
});
