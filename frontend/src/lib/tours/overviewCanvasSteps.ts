/**
 * The first half of the *Overview* tour (#911, spec #910): the visit of the
 * canvas, on the tour's own finished Run — open it, the three panes, implementer
 * and its output and terminal, the reviewer → End edge, Start. The panel each
 * gesture opens stays lit and explorable but read-only, and a few steps pin side
 * bubbles on what it shows. The tour's definition, and its second half (the
 * left panel's tabs, archiving, Settings and Stats), are in `overview.ts`.
 */

import type { TourStep } from "../tourDef";
import { TUTORIAL_OVERVIEW_PIPELINE_ID, overviewRunName } from "./overviewPipeline";
import { overviewRunId } from "./overviewPreparation";
import {
  CANVAS_PANEL,
  CODE_ROW,
  CODE_VIEWER,
  EDGE_CONDITION,
  INSPECTOR_EXPLORE,
  INSPECTOR_TAB_CONFIG,
  INSPECTOR_TAB_RUN,
  LEFT_PANEL,
  READING_TIMEOUT_MS,
  RIGHT_PANEL,
  RUNS_TAB,
  START_INPUT,
  TASK_ROW,
  TERMINAL,
  TERMINAL_RESTORE,
  endEdgeIndex,
  endEdgeOpen,
  implementerOpen,
  marker,
  nodeSel,
  onConfigTab,
  readStep,
  runIsOpen,
  runRow,
  selectedNode,
  startOpen,
  worker,
} from "./overviewReading";

const testId = (id: string) => `[data-testid="${id}"]`;

export const CANVAS_STEPS: TourStep[] = [
  {
    id: "open-run",
    title: "Open your Run",
    body: (o) =>
      runIsOpen(o.app)
        ? `\`${overviewRunName()}\` is open. Every Run is a row in this list; clicking one opens it on the canvas.`
        : `Click \`${overviewRunName()}\` in the list. The tour just ran it for you: a real Run of the ${TUTORIAL_OVERVIEW_PIPELINE_ID} pipeline, already finished.`,
    // The row only exists on the Runs tab: a reader who is elsewhere is sent there.
    target: (o) => {
      const id = overviewRunId();
      if (!id) return [];
      return o.present(runRow(id)) ? [runRow(id)] : [RUNS_TAB];
    },
    waitingFor: "your Run in the list on the left",
    failureHint: "The list may be filtered — clear the filters above it.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the Run opens",
    done: (o) => runIsOpen(o.app),
  },
  readStep({
    id: "canvas",
    title: "This is the canvas",
    body: "A pipeline is a succession of nodes linked by edges, drawn here. This one hands a task from Start to implementer, then reviewer, then End.",
    target: () => [CANVAS_PANEL],
    soft: true,
    waitingFor: "the canvas",
  }),
  readStep({
    id: "left-panel",
    title: "The left panel",
    body: "Your Runs live here, newest first, next to the Triggers and the Pipelines tabs. Clicking a row opens it on the canvas.",
    target: () => [LEFT_PANEL],
    waitingFor: "the left panel",
  }),
  readStep({
    id: "right-panel",
    title: "The right panel",
    // PDO-v2 #43: with nothing selected, the pane shows Pipeline info (the Run
    // and its pipeline); on a pipeline tab with nothing selected it folds away.
    body: "The details of whatever is selected: a node, an edge. With nothing selected it shows Pipeline info, the Run and its pipeline; on a pipeline it folds away instead.",
    target: () => [RIGHT_PANEL],
    waitingFor: "the right panel",
  }),
  {
    id: "open-implementer",
    title: "Click implementer",
    body: (o) =>
      selectedNode(o.app, worker(o.app, "implementer"))
        ? "A node is usually an agent running in a harness (Claude Code, Copilot, …); here it is a script, so the tour costs nothing. It takes an input and produces outputs. Its Config tab shows what it runs with, prompt and skills, read-only: following a Run never changes it."
        : "Click the implementer card. A node is usually an agent running in a harness (Claude Code, Copilot, …); this one is a script, so the tour costs nothing.",
    target: (o) => nodeSel(worker(o.app, "implementer")),
    waitingFor: "the implementer node on the canvas",
    failureHint: "It sits right under Start; scroll or zoom the canvas to bring it into view.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the inspector shows the node",
    // Once clicked the card stops for a Next: the body just changed to say what
    // a node is, and advancing at once would flash it past (FP #911).
    confirm: (o) => implementerOpen(o),
    done: (o) => implementerOpen(o),
    // The inspector it opened stays bright, to look around in — not to edit.
    readOnly: (o) => (implementerOpen(o) ? [RIGHT_PANEL] : []),
    explore: INSPECTOR_EXPLORE,
    asides: () => [
      { target: TASK_ROW, text: "Its input, task: what the edge from Start delivered — the Run's prompt." },
      {
        target: INSPECTOR_TAB_CONFIG,
        text: "Config: the node's prompt and skills, read-only. To change a Run, use the Edit pencil in the canvas toolbar.",
      },
    ],
  },
  {
    id: "outputs",
    title: "Open its output",
    // A gesture now (#911, human test): the reader clicked `code` anyway, and a
    // tour that did not notice walked on to the terminal behind an open viewer.
    body: (o) => {
      if (o.present(CODE_VIEWER)) {
        return "This is code, the file implementer wrote — and exactly what reviewer received through their edge.";
      }
      return onConfigTab(o)
        ? "Click the Run tab: a finished node's outputs are listed there."
        : "A finished node's outputs are listed here: implementer wrote one, code. Click code to read it.";
    },
    // Re-aims onto the viewer the click opened, lit as a page to read — the way
    // *First run*'s last step re-aims onto its `out` file.
    target: (o) => {
      if (o.present(CODE_VIEWER)) return [CODE_VIEWER];
      return onConfigTab(o) ? [INSPECTOR_TAB_RUN] : [CODE_ROW];
    },
    soft: (o) => o.present(CODE_VIEWER),
    waitingFor: "the outputs of implementer",
    failureHint: "They are in the node's inspector on the right, under Outputs.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the output opens",
    confirm: (o) => o.present(CODE_VIEWER),
    done: (o) => o.present(CODE_VIEWER),
    // Until then the inspector is lit to explore; once the viewer is up, its
    // backdrop covers the panel and lighting it would only expose that.
    readOnly: (o) => (o.present(CODE_VIEWER) ? [] : [RIGHT_PANEL]),
    explore: INSPECTOR_EXPLORE,
  },
  {
    id: "output-viewer",
    title: "Close the output",
    body: "Outputs open in this viewer, markdown rendered; a looped node's iterations page through here too. Close it with ✕ or Escape to go on.",
    target: () => [CODE_VIEWER],
    soft: true,
    waitingFor: "the viewer of the code output",
    failureHint: "It opens from the code row, under Outputs in the node's inspector.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the viewer closes",
    // Closing the viewer destroys the target — the condition is read first, so
    // that is the step done, not a missing target (`observeTour`).
    done: (o) => !o.present(CODE_VIEWER),
    // That condition holds for good once the viewer is closed, but « close it »
    // only makes sense with the viewer open: `Previous` from the terminal step
    // is greyed out rather than replaying it over a viewer that is gone
    // (CONTEXT.md § « Rejeu d'étape »).
    replayNeedsTarget: true,
  },
  {
    id: "terminal",
    title: "Its terminal",
    // A finished node opens with its terminal folded to a bar (#346), so the
    // step is a gesture until it is unfolded — then a card to read, with Next.
    body: (o) => {
      if (o.present(TERMINAL)) {
        return "Every node runs in its own terminal; this one is frozen on the lines the script printed. While a node runs it is live and interactive — you talk to the harness there, and First run shows you how.";
      }
      return onConfigTab(o)
        ? "Click the Run tab: every node runs in its own terminal, and it is shown there."
        : "Click the ended session bar to unfold its terminal. Every node runs in its own terminal; a finished node's is folded to leave room for its outputs.";
    },
    target: (o) => {
      if (o.present(TERMINAL)) return [TERMINAL];
      if (onConfigTab(o)) return [INSPECTOR_TAB_RUN];
      return o.present(TERMINAL_RESTORE) ? [TERMINAL_RESTORE] : [TERMINAL];
    },
    waitingFor: "the node's terminal",
    failureHint: "The terminal is in the Run tab of the node's inspector.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the terminal opens",
    confirm: (o) => o.present(TERMINAL),
    done: (o) => o.present(TERMINAL),
    readOnly: () => [RIGHT_PANEL],
    explore: INSPECTOR_EXPLORE,
  },
  {
    id: "open-end-edge",
    title: "Click the edge reviewer → End",
    body: (o) =>
      o.app.selection.kind === "edge" && o.app.selection.edgeIndex === endEdgeIndex(o.app)
        ? "An edge carries conditions on its source's outputs and fires when the source finishes. This one reads verdict = pass; the loop back to implementer reads verdict = fail."
        : "Click the edge from reviewer down to End. An edge carries conditions on its source's outputs, and fires when the source finishes.",
    // The edge and its condition pill: a straight edge has a zero-wide box, and
    // the pill — which selects the edge too — gives the hole something to hold.
    target: (o) => {
      const i = endEdgeIndex(o.app);
      if (i < 0) return [];
      const edge = `.react-flow__edge[data-id="e-${i}"]`;
      const pill = testId(`edge-condition-label-e-${i}`);
      return o.present(pill) ? [edge, pill] : [edge];
    },
    waitingFor: "the reviewer → End edge",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when the edge is selected",
    confirm: (o) => endEdgeOpen(o),
    done: (o) => endEdgeOpen(o),
    readOnly: (o) => (endEdgeOpen(o) ? [RIGHT_PANEL] : []),
    asides: () => [
      { target: EDGE_CONDITION, text: "The condition: this edge fires only when reviewer's review says verdict = pass." },
    ],
  },
  {
    id: "open-start",
    title: "Click Start",
    // The why before the click (#911, human test): the reader clicked Start
    // without knowing what for, then could not find the prompt it opened.
    body: (o) =>
      selectedNode(o.app, marker(o.app, "start"))
        ? "Start and End are special nodes: Start holds the prompt this Run was started with, shown on the right, and End is where a Run stops."
        : "Click Start at the top to see the prompt this Run was started with. Start and End are special nodes: Start carries the Run's input, End is where a Run stops.",
    target: (o) => nodeSel(marker(o.app, "start")),
    waitingFor: "the Start node on the canvas",
    targetTimeoutMs: READING_TIMEOUT_MS,
    advanceHint: "advances when Start is selected",
    confirm: (o) => startOpen(o),
    done: (o) => startOpen(o),
    readOnly: (o) => (startOpen(o) ? [RIGHT_PANEL] : []),
    asides: () => [
      {
        target: START_INPUT,
        text: "The input this Run was started with — the prompt typed in New Run. Start hands it to implementer through their edge.",
      },
    ],
  },
];
