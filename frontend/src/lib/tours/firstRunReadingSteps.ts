/**
 * The reading half of the *First run* tour (#825): after Launch, the life of ONE
 * interactive node — find the Run, open the node, talk to it, release it, wait
 * for it, open its output. Pure data, spliced after the form steps in
 * `firstRun.ts`; the copy rules there apply here too.
 */

import { isNodeFinished } from "../tourDef";
import type { TourStep } from "../tourDef";
import {
  TUTORIAL_OUT_PORT,
  TUTORIAL_TERMINAL_LINE,
} from "./firstRunPreparation";
import {
  APPROVE_MENU,
  AUTO_APPROVE_ENTRY,
  INSPECTOR_RUN,
  OUT_MODAL,
  OUT_ROW,
  READING_TIMEOUT_MS,
  RELEASE_BUTTON,
  TERMINAL,
  endedBadly,
  followedNode,
  inspectorShows,
  nodeSel,
  runIsOpen,
  runRowSel,
} from "./firstRunReading";

export const READING_STEPS: TourStep[] = [
  // ---- reading the Run (#825) ---------------------------------------------
  // Launch closed the modal. From here the tour teaches the life of ONE
  // interactive node, and every step advances on the Run's own state — the
  // status the inspector polls, the guard flag, the artifact on disk — never on
  // a timer. Each is skippable: a reader who already knows this half should not
  // have to sit through it to get their checkmark.
  {
    id: "find-run",
    title: "Find your Run",
    // Launch opens the Run's tab itself, so on the nominal path the condition
    // goes true a tick after this step is entered — too late for
    // `satisfiedOnEntry`, and a step that advanced on its own was therefore
    // never seen by anybody (the FP of #825 watched the tour jump from 9 to 11).
    // It still has an idea to give — the list on the left is where your Runs
    // live — so it stops for a `Next` once the Run is open, and says what
    // already happened rather than instructing a click nobody needs to make.
    //
    // No « top row »: on a shared instance a neighbour who launched a second
    // later sits above yours. The card names your Run, the frame finds it
    // wherever it is, and both sentences stay true at any position.
    body: (o) => {
      const name = o.app.myRun?.name ?? "your Run";
      return runIsOpen(o.app)
        ? `Your Run, ${name}, is lit in the list on the left, and Launch already opened its tab. Every Run is a row here; the icon is its status.`
        : `Click ${name} in the list on the left. Every Run is a row here; the icon is its status.`;
    },
    // Strictly the row. Falling back to the Runs tab would keep a target alive
    // forever, and « waiting on something that will never come » is precisely
    // what story 18 asks a tour not to do.
    target: (o) => {
      const row = runRowSel(o.app);
      return row ? [row] : [];
    },
    waitingFor: "your Run in the list on the left",
    failureHint: "The list may be filtered — clear the filters above it, or reopen the Runs tab.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances when the Run opens",
    // Only once it IS open: while the reader still has the click to make, the
    // footer says what the tour is waiting for, and a `Next` nobody can press
    // would be a control in the way of the instruction.
    confirm: (o) => runIsOpen(o.app),
    // The Run's tab is open on the canvas — which is what clicking the row does.
    done: (o) => runIsOpen(o.app),
  },
  {
    id: "open-node",
    // Opening a Run puts the selection on its live node, so on the nominal path
    // this step is entered already satisfied and stops for a `Next` — the same
    // shape as `find-run` above. It says so rather than instructing a click that
    // would change nothing, which a reader reads as an overlay swallowing their
    // click (FP finding, #825).
    title: "Open the node",
    body: (o) => {
      const name = followedNode(o.app)?.name ?? "agent";
      return inspectorShows(o, followedNode(o.app))
        ? `The inspector on the right is already showing the ${name} node — opening a Run selects the node it is running. It is the one agent of this pipeline, and that pane is its live terminal.`
        : `Click the ${name} node. It is the one agent of this pipeline; the inspector on the right shows its live terminal.`;
    },
    target: (o) => nodeSel(followedNode(o.app)),
    waitingFor: "the agent node on the canvas",
    failureHint: "It sits between Start and End; scroll or zoom the canvas to bring it into view.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances when the inspector shows the node",
    // No `confirm`: a reader who still owes the click should have it advance the
    // moment they make it. The `Next` on this card comes from `satisfiedOnEntry`,
    // which is the case the branched body above is written for.
    done: (o) => inspectorShows(o, followedNode(o.app)),
  },
  {
    id: "talk-to-agent",
    title: "Talk to the agent",
    body: "Click in the terminal and paste the line, then press Enter. This is the agent's own session; if it first asks whether you trust the folder, answer yes.",
    target: () => [TERMINAL],
    waitingFor: "the agent's terminal in the Run inspector",
    failureHint: "The terminal is in the Run tab of the inspector, above the buttons.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    copyBlock: TUTORIAL_TERMINAL_LINE,
    skippable: true,
    // No condition at all: what happens in tmux is a pane of text, not a form
    // control, and a tour that claimed to read it would be guessing. `Next` is
    // live from the start — the user owns this one (story 14).
  },
  {
    id: "release-completion",
    title: "Release the completion",
    body: "Open the menu of Approve ▾ and choose Auto-approve when ready. An interactive node is guarded: the agent cannot finish until you release it — this is the release.",
    note: "Approve itself completes the node now, with the outputs as they are — not for now.",
    // The chevron while the menu is shut, its entry once it is open (a portal):
    // then a soft zone, so the entry stays clickable.
    target: (o) => (o.present(APPROVE_MENU) ? [AUTO_APPROVE_ENTRY] : [RELEASE_BUTTON]),
    soft: (o) => o.present(APPROVE_MENU),
    waitingFor: "the Approve ▾ button",
    failureHint: "It sits at the right of the I/O and Transcript tabs while the node is live.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances once released",
    // A node that has already stopped is past this step whatever opened its
    // guard — including the other button. Reading only the flag would stop the
    // tour on a card about a gesture that no longer has a button to make it.
    done: (o) => {
      const node = followedNode(o.app);
      return node != null && (node.released || isNodeFinished(node));
    },
  },
  {
    id: "wait-for-node",
    title: "Wait for the node to finish",
    // Nothing left to instruct once it ended badly: the checklist has shown what
    // happened and the note below says what it means.
    body: (o) =>
      endedBadly(followedNode(o.app))
        ? ""
        : "The agent writes its output and completes. Watch the status pill at the top of the inspector turn from Running to Completed.",
    // The nominal card used to open on « Nothing to do », and that is not always
    // true: an agent quick enough to run `pdo complete` before the release is
    // refused, and parks on a question — the node then ends only once somebody
    // answers it (FP finding, #825). Ordinary, not exotic, and it lands in the
    // first five minutes of a newcomer. The terminal is inside the soft zone, so
    // they can answer from where they are standing.
    note: (o) =>
      endedBadly(followedNode(o.app))
        ? "The node ended without completing. Its terminal above says why; the tour goes on to the output, which may be missing."
        : "If the agent asks you something in the terminal above, answer it — the step waits for as long as it takes.",
    // The whole inspector, as a zone rather than a target: during a wait of
    // unknown length the reader must keep the terminal — read it, scroll it,
    // answer a late question. The rest of the app stays blocked.
    target: () => [INSPECTOR_RUN],
    soft: true,
    // The one step with no limit (ticket: « sans limite de temps »). An agent
    // takes as long as it takes, and a countdown here would be the tour deciding
    // how fast somebody else's work should be.
    targetTimeoutMs: null,
    waitingFor: "the Run inspector",
    waitingNote: "Waiting for the node to finish · no time limit",
    skippable: true,
    // Skipping a wait means "I have seen enough" (design Q3). The steps after it
    // are about a file the node has not written, so the recap is the honest
    // place to land — and it says the node is still running.
    skipEndsTour: true,
    advanceHint: "advances on its own",
    // A failure is still an ending, so the step is done — but it waits for a
    // click, because the sentence explaining it is the point.
    confirm: (o) => endedBadly(followedNode(o.app)),
    done: (o) => isNodeFinished(followedNode(o.app)),
    checklist: (o) => {
      const node = followedNode(o.app);
      const finished = isNodeFinished(node);
      return [
        {
          label: "completion released",
          done: node?.released === true,
          // The engine has no « back ». A reader who skipped the release learns
          // here why nothing is happening — and the button is inside the soft
          // zone, so they can still press it.
          note:
            node?.released || finished
              ? undefined
              : "the node waits for it: choose Auto-approve when ready under Approve ▾",
        },
        {
          label: "node finished",
          done: finished,
          badge: endedBadly(node) ? (node?.status ?? undefined) : undefined,
        },
      ];
    },
  },
  {
    id: "open-output",
    title: "Open the output",
    // Two beats, one card: click the row, then read what opened.
    body: (o) =>
      o.present(OUT_MODAL)
        ? "This is what the agent wrote — the summary your prompt asked for, and the only thing the next node would have received. Read it, then press Next."
        : `Click ${TUTORIAL_OUT_PORT}. It is the file the agent wrote — the summary your prompt asked for — and the only thing the next node would have received.`,
    // Re-aims onto the artifact the click opened, the way `pick-repo` re-aims
    // onto the explorer. Without it the tour ends the instant the file appears,
    // and the recap card lands on top of the summary it just told the reader to
    // open — the one thing this step exists to show them (FP finding, #825).
    target: (o) => (o.present(OUT_MODAL) ? [OUT_MODAL] : [OUT_ROW]),
    // A page to read, not a control to hit: once the artifact is up it is lit as
    // a zone — dashed, lighter dim, scrollable — and the popover moves beside it.
    soft: (o) => o.present(OUT_MODAL),
    waitingFor: `the ${TUTORIAL_OUT_PORT} row under Outputs`,
    failureHint: "A node that failed or was stopped writes no output.",
    targetTimeoutMs: READING_TIMEOUT_MS,
    skippable: true,
    advanceHint: "advances when the file opens",
    // The reader owns the last moment of the tour: once the file is up, `Next`
    // is what goes to the recap — so the recap cannot land on the summary
    // before it has been read. Until then the footer keeps its waiting line.
    confirm: (o) => o.present(OUT_MODAL),
    done: (o) => o.present(OUT_MODAL),
  },
];
