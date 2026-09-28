/**
 * What the *First run* tour (#824) stands on: the training repository, the
 * one-node `tutorial-interactive` pipeline, the blocks its steps ask the user to
 * paste, and the two idempotent preparations that create the first two.
 *
 * The two preparations go through verbs that know nothing about tours (ADR-0071
 * §3): a generic repo-creation verb, and the ordinary pipeline API. Both are
 * reused when what they make is already there, so replaying the tour is free and
 * a user who edited the training repo keeps their edit.
 */

import { ApiError } from "../../shared/http/client";
import { createPipeline, fetchPipelines, savePipeline } from "../../pipeline/api/pipelines";
import { createRepo } from "../../workspace/api/repos";

/** Where the training repository is created, and the folder the explorer opens on. */
export const TUTORIAL_REPO_PARENT = "/tmp";
export const TUTORIAL_REPO_NAME = "pdo-tutorial";
export const TUTORIAL_REPO_PATH = `${TUTORIAL_REPO_PARENT}/${TUTORIAL_REPO_NAME}`;

/** The one-node pipeline the tour launches. A normal Library pipeline: no prefix,
 *  no tag — the name already says what it is (design Q8). */
export const TUTORIAL_RUN_PIPELINE_ID = "tutorial-interactive";

/** The block step 8 asks the user to paste. Deliberately concrete: the agent edits
 *  `notes.txt` and writes its output file, which the reading tour then shows. */
export const TUTORIAL_PROMPT =
  "Add a line saying hello from PDO to notes.txt, then write a short summary of what you did in your output file.";

/** The line step 12 asks the user to type INTO the agent's own session (#825).
 *  An interactive node waits for a human; this is what telling it "go ahead"
 *  looks like, in the agent's language rather than the app's. */
export const TUTORIAL_TERMINAL_LINE = "I am done, please write your summary and finish.";

/** The output port the training pipeline declares — the file step 15 opens. */
export const TUTORIAL_OUT_PORT = "out";

const TUTORIAL_OUT_EXPECTED = "Summary of what you changed";

const README = `# pdo-tutorial

A throwaway repository PDO created for the First run tour. Delete it whenever you like.
`;

const NOTES = `Notes\n`;

/**
 * The training pipeline's document: one interactive node between Start and End,
 * no harness pinned (Inherit), one markdown output that says what it must contain.
 *
 * Interactive is the whole point — the node waits for the user instead of running
 * to completion on its own, which is what makes the Run worth looking at.
 *
 * All three nodes carry a `view` (#825, FP iteration 2). Only `assistant` used to,
 * so the canvas laid Start and End out itself — in a column at x = 200, straight
 * under the agent at x = 260. The very first canvas a newcomer sees had two cards
 * on top of each other. Three cards in a row, a step apart, is what the graph
 * actually is.
 */
const TUTORIAL_PIPELINE_YAML = `name: ${TUTORIAL_RUN_PIPELINE_ID}
version: "1.0"

variables: {}

nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - name: user_prompt
    view: { x: 80, y: 120 }
  - id: assistant
    name: assistant
    type: agent
    interactive: true
    isolated_worktree: false
    inputs:
      - name: task
    outputs:
      - name: out
        port_type: markdown
        instructions: ${TUTORIAL_OUT_EXPECTED}
    view: { x: 360, y: 120 }
  - id: end
    name: End
    type: end
    inputs:
      - name: result
    view: { x: 640, y: 120 }

edges:
  - source: { node: start, port: user_prompt }
    target: { node: assistant, port: task }
  - source: { node: assistant, port: out }
    target: { node: end, port: result }
`;

const TUTORIAL_NODE_PROMPT = `Do what the user asked, in the repository you are in.
Then write a short summary of what you changed into your output file.`;

// ---- preparation ----------------------------------------------------------

export async function prepareRepo(): Promise<void> {
  await createRepo(TUTORIAL_REPO_PARENT, TUTORIAL_REPO_NAME, [
    { path: "README.md", content: README },
    { path: "notes.txt", content: NOTES },
  ]);
}

/**
 * Put `tutorial-interactive` in the Library, or leave the one that is already
 * there alone — **even if the user edited it** (design §1). Re-writing it would
 * quietly undo their change, and a tutorial pipeline is theirs once it exists.
 *
 * The existence check is the pipeline list rather than the create call's 409,
 * because a 409 also fires on a name that collides for another reason; the list
 * answers the question that is actually being asked.
 */
export async function prepareTutorialPipeline(): Promise<void> {
  const existing = await fetchPipelines();
  if (existing.some((p) => p.id === TUTORIAL_RUN_PIPELINE_ID)) return;
  try {
    await createPipeline(TUTORIAL_RUN_PIPELINE_ID);
  } catch (e) {
    // Lost the race with another tab (or a list that was a beat stale): the
    // pipeline exists, which is all this preparation promised.
    if (!(e instanceof ApiError) || e.status !== 409) throw e;
    return;
  }
  await savePipeline(TUTORIAL_RUN_PIPELINE_ID, TUTORIAL_PIPELINE_YAML, {
    assistant: TUTORIAL_NODE_PROMPT,
  });
}
