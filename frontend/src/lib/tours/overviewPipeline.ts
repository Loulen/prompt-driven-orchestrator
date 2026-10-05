/**
 * What the *Overview* tour (#911, spec #910) runs: the `tutorial-overview`
 * pipeline — `implement-review`'s layout with two `script` nodes (ADR-0017) in
 * place of the agents, so the tour needs no harness — the Run's prompt, and the
 * names the preparations give what they create. The preparations themselves
 * live in `overviewPreparation.ts`.
 */

/** The pipeline the tour's Run executes. A normal Library pipeline, kept after the tour. */
export const TUTORIAL_OVERVIEW_PIPELINE_ID = "tutorial-overview";

/** The name the tour gives its Run, so the list says what the row is. */
export const OVERVIEW_RUN_NAME = "tour-overview";

/** The Run's prompt — fixed, and what the Start step shows the reader. */
export const OVERVIEW_PROMPT =
  "Show me around PDO: implement a tiny change, then review it and hand back a verdict.";

/** The Trigger the tour installs to be shown, and removes on the way out. */
export const EXAMPLE_TRIGGER_NAME = "tutorial-example-trigger";

/** Every 15 minutes — a schedule anyone can read at a glance. */
export const EXAMPLE_TRIGGER_CRON = "*/15 * * * *";

/**
 * The guard that makes the Trigger harmless **by construction** (ADR-0071 §4):
 * `false` exits non-zero, so every fire is skipped and no Run is ever created —
 * even when a tab closed mid-tour left the Trigger behind.
 */
export const EXAMPLE_TRIGGER_GUARD = "false";

export const EXAMPLE_TRIGGER_INPUT =
  "An example input. This Trigger's guard refuses every fire, so it never starts a Run.";

// ---- the pipeline -----------------------------------------------------------

/**
 * `implement-review`'s layout (the README media fixture: positions, anchors, edge
 * routes), with two `script` nodes in place of the agents. Scripts run in the
 * Run worktree and only ever write their outputs, so the worktree stays clean.
 * No `image_list` port: a script has nothing to take a screenshot of.
 *
 * Edge order matters to the tour: the canvas keys an edge by its index, and the
 * edge step points at reviewer → End (`verdict eq pass`), the one the Run takes.
 */
export const OVERVIEW_PIPELINE_YAML = `name: ${TUTORIAL_OVERVIEW_PIPELINE_ID}
version: "1.0"
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: user_prompt, side: right }
    view: { x: 199, y: 47 }
  - id: end
    name: End
    type: end
    inputs:
      - { name: result, side: left }
    view: { x: 204, y: 316 }
  - id: implementer
    name: implementer
    type: script
    isolated_worktree: false
    inputs:
      - { name: task, side: left }
    outputs:
      - name: code
        side: right
        instructions: "What was changed, in a few lines."
    view: { x: 196, y: 135 }
  - id: reviewer
    name: reviewer
    type: script
    isolated_worktree: false
    inputs:
      - { name: code, side: left }
    outputs:
      - name: review
        side: right
        frontmatter:
          verdict:
            allowed: [pass, fail]
            type: enum
        instructions: "Your verdict, then a small Mermaid diagram of the flow you checked."
    view: { x: 197, y: 208 }
edges:
  - source: { node: start, port: user_prompt }
    target: { node: implementer, port: task }
    mode: manual
    waypoints:
      - { x: 280, y: 95 }
    target_side: top
    source_anchor: { side: bottom, offset: 81 }
    target_anchor: { side: top, offset: 84 }
  - source: { node: implementer, port: code }
    target: { node: reviewer, port: code }
    mode: manual
    waypoints:
      - { x: 280, y: 168 }
    target_side: top
    source_anchor: { side: bottom, offset: 84 }
    target_anchor: { side: top, offset: 83 }
  - source: { node: reviewer, port: review }
    target: { node: implementer, port: task }
    when:
      verdict: { eq: fail }
    mode: manual
    waypoints:
      - { x: 397, y: 154 }
      - { x: 396, y: 154 }
    target_side: right
    source_anchor: { side: right, offset: 26 }
    target_anchor: { side: right, offset: 19 }
    output_label_pos:
      review: { x: 381, y: 225 }
    condition_label_pos: { x: 395, y: 192 }
  - source: { node: reviewer, port: review }
    target: { node: end, port: result }
    when:
      verdict: { eq: pass }
    mode: manual
    waypoints:
      - { x: 280, y: 276 }
    target_side: top
    source_anchor: { side: bottom, offset: 83 }
    target_anchor: { side: top, offset: 76 }
    output_label_pos:
      review: { x: 253, y: 256 }
    condition_label_pos: { x: 284, y: 284 }
loops:
  - id: loop-tutorial-overview
    kind: bounded
    members: [implementer, reviewer]
    max_iter: 3
`;

/**
 * The implementer's bash body (ADR-0017: stored in the node's prompt slot, run as
 * `timeout 60s bash <body>`). It prints what the frozen terminal pane will show,
 * and writes its one output.
 *
 * It completes **itself**, from inside its session, and waits there to be reaped
 * (#911). The daemon snapshots a node's pane when it reaps the session, after a
 * completion; the wrapper's own `pdo complete` runs only once the script has
 * exited, and the session goes with it — so a script that simply ended would
 * leave the terminal step a « Session no longer available ». Should the reap be
 * slow, the wait gives up after 20 s, the wrapper completes again, and the
 * daemon answers it as a legal duplicate.
 */
export const IMPLEMENTER_SCRIPT = `#!/usr/bin/env bash
# tutorial-overview · implementer — a script node, so the tour needs no agent.
set -euo pipefail

echo "implementer: reading the task"
echo "implementer: an agent would edit the repo;"
echo "implementer: this script only writes output"
sleep 1

cat > "$PDO_OUTPUT_CODE" <<'OUT'
# What the implementer did

Nothing to the repository: this node is a script, so the tour costs nothing.
An agent node would have made the change here and described it in this file.
OUT

echo "implementer: wrote code — done"
# Hand the output over from inside the session, then wait to be reaped. The
# daemon freezes a node's pane when it reaps the session; the wrapper's own
# \`pdo complete\` only runs once this script has exited — and its session with
# it — so the tour's terminal step would have nothing to show.
pdo complete
# Then leave as soon as the pane is gone — polled, not slept: the reap hangs up
# the session's shell only, and a plain \`sleep\` would outlive it (20 s at most).
for _ in $(seq 1 100); do
  tmux display-message -p -t "\${TMUX_PANE:-}" "" >/dev/null 2>&1 || exit 0
  sleep 0.2
done
`;

/** The reviewer's body: `verdict: pass` in the frontmatter, and a mermaid diagram. */
export const REVIEWER_SCRIPT = `#!/usr/bin/env bash
# tutorial-overview · reviewer — a script node, so the tour needs no agent.
set -euo pipefail

echo "reviewer: reading the code output"
echo "reviewer: verdict pass — the edge to End"
echo "reviewer: fires, the Run ends"
sleep 1

cat > "$PDO_OUTPUT_REVIEW" <<'OUT'
---
verdict: pass
---
# Review

The change holds up. The edge reviewer → End reads this verdict and ends the Run.

\`\`\`mermaid
flowchart LR
  Start --> implementer --> reviewer
  reviewer -- verdict eq pass --> End
  reviewer -- verdict eq fail --> implementer
\`\`\`
OUT

echo "reviewer: wrote review — done"
# Hand the output over from inside the session, then wait to be reaped. The
# daemon freezes a node's pane when it reaps the session; the wrapper's own
# \`pdo complete\` only runs once this script has exited — and its session with
# it — so the tour's terminal step would have nothing to show.
pdo complete
# Then leave as soon as the pane is gone — polled, not slept: the reap hangs up
# the session's shell only, and a plain \`sleep\` would outlive it (20 s at most).
for _ in $(seq 1 100); do
  tmux display-message -p -t "\${TMUX_PANE:-}" "" >/dev/null 2>&1 || exit 0
  sleep 0.2
done
`;
