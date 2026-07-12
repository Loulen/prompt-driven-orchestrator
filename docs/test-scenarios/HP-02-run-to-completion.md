---
id: HP-02
covers: [run, start-node, tmux-session, dataflow, conditional-routing, loop-region, collection, merge, artifact, run-stats]
---

# HP-02 — Launch a run to completion

## Goal

A user launches a **Run** on a pipeline: picks a target repo, enters a prompt (optionally images),
and watches nodes spawn real **tmux sessions** running `claude`, data route through edges / a loop
region / a collection fan-out into a **Merge**, until the Run reaches a clean **Completed** state with
inspectable artifacts and live stats — the core "drive an orchestration to its end" loop.

## Drive-by

Features validated while crossing the run screens (grafted from retired per-issue scenarios):

- **Repo explorer**: the loupe opens a filesystem explorer (dirs only, dotfiles hidden, git/symlink
  flags) that picks a folder through the existing validation path and degrades gracefully on an
  unreadable directory (#131).
- **Start-node input images**: images surface on the Start node and in the Start inspector (#145).
- **Conditional routing**: a producer fans out to **all** matching guarded edges (multi-match); an
  `else` edge catches the unmatched case; pills are always visible (#144).
- **Loop region**: a bounded review loop exits early on its PASS edge and, if the verdict never
  passes, halts explicitly **"exhausted — unrouted"** (never a silent stall) (#148).
- **Collection fan-out / Merge**: a `kind: collection` region fans its (single-member, #269 /
  ADR-0026) body out in parallel — one lap per item — the barrier fires once when all laps finish,
  and an empty collection fires the barrier immediately (#151, ADR-0006).
- **Artifact rendering**: an output artifact opens in the markdown modal; a ` ```mermaid ` block
  renders as inline SVG and invalid mermaid degrades gracefully to raw source (#240 / ADR-0013).
- **Run stats**: the Info panel shows a Stats block — Duration (ticking live), Node sessions started
  (manager excluded), Lines changed / LOC — and an **estimated cost** (labelled "est.", #100 / #272).
- **Runs / Triggers grouped by repo** when ≥ 2 distinct repos are present; flat otherwise (#258).
- **Daemon version** displayed live in the footer (#139).

## Preconditions

- The app is running locally and reachable in a browser; status bar shows the daemon **connected**.
- `claude` is on `PATH` (the daemon shells out to it for each node session).
- A valid pipeline and a target git repo are available. No hard-coded ports/ids in the journey — see
  `docs/agents/run-scenario.md` for how to drive PDO and probe side-effects.

## Journey

1. Open the **New Run** modal → **pick a repo** (use the explorer loupe to browse and select a git
   repo; the field validates with a green border + branch loading).
2. Choose a pipeline, **enter a prompt** (optionally attach an input image) → **Launch**; capture the
   resulting run.
3. The canvas shows the **Start node** (▶) and **End node** (◯) with the dataflow between them; within
   a couple of seconds the first work node animates to **running**.
4. Select the running node → the right panel shows a **live terminal preview** (real `claude` TUI,
   wrapping without horizontal scroll) and the deterministic prompt preamble (`## Inputs` / `## Outputs`).
5. Data routes downstream: conditional edges fire to all matching targets, the loop region iterates and
   exits on PASS, a collection region fans out in parallel and converges on the **Merge**.
6. The Run reaches **Completed** (the happy ending): nodes read completed, the End inspector shows the
   `result` port **received**.
7. Open an output artifact → the **markdown modal** renders it (including a mermaid diagram as SVG).
8. Open the Run **Info panel** → the **Stats** block shows Duration, Node sessions started, Lines
   changed / LOC, and an **estimated cost** ("Est. cost", labelled as an estimate — "—" when uncomputable).
9. Find the run in the **Runs list** (grouped by repo when ≥ 2 repos exist).

## Checks

### UI

- Start/End nodes render; the running node shows live, wrapping terminal output (no horizontal scrollbar).
- Routing matches the pipeline shape (multi-match fan-out, loop `↻ X/Y` header iterating, collection
  `⇉ N items` badge, Merge convergence).
- The Run settles to **Completed**; the End `result` port shows **received**.
- The artifact modal shows the content; a valid mermaid block is an SVG, an invalid one falls back to
  `<pre><code>` (never a blank pane, never a thrown error).
- Stats: Duration ticks on a live run, freezes on a terminal one; an **estimated cost** ("Est. cost",
  framed as an estimate) is shown, "—" when uncomputable.

### Backing store

- A tmux session named for the run/node/iter is alive while the node runs and shows `claude` (not a
  bare shell); the output artifact file exists on disk under the run worktree.
- The Run's projected state and stats agree with daemon ground truth (run endpoint: `sessions_spawned`,
  `started_at`/`completed_at`, `loc`) and with git (`diff --numstat`, `.pdo/` excluded).

## Cleanup (best-effort)

- Archive the Run (`cleanup_run`): it reaps sessions and the worktree. Delete any pipeline the agent
  seeded.

## Notes

- **The clean-terminal-state check here is the happy ending — not adversity.** Deep failure modes
  (daemon kill, session death, admission-slot leak, mid-run-edit rejection) are edge cases covered by
  the layer-3 automated tests, **not** by this HP.
- **Select data by characteristics, not hard-coded ids** (HP mode): if no data satisfies a condition,
  that is a legitimate finding, not an excuse to bypass the UI.
- A first `claude` launch in a fresh worktree lands on the trust dialog — confirm it (see the driving
  playbook) before expecting chat output.
- A node with no output yet returns **409 `missing_outputs`** on "Mark complete" — that guard is
  expected, not a bug.
