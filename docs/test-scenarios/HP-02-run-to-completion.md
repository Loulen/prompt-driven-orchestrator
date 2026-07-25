---
id: HP-02
covers: [run, start-node, tmux-session, dataflow, conditional-routing, loop-region, collection, merge, artifact, run-stats, sandbox, staging-profile, staging-floor, sandbox-prep]
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
- **Sandbox, as an A/B pair** (PRD #403, ADR-0030, ADR-0031): the same one-node pipeline is launched
  twice — once in staging profile **`full`**, once **`off`** — and the two Runs must reach the same
  business outcome by two visibly different routes. The `off` twin is the **control**: without it, a
  green sandboxed Run proves nothing (a Run that silently fell back to the host path also looks
  green). See the journey's §10-12 and the dedicated checks below.

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
10. **Sandboxed twin.** Seed (or reuse) a **one-node pipeline** whose node asks for a single line of
    output and then completes. Open **New Run** on it → the sandbox field offers `off`, `minimal`,
    `full` and any named **staging profile**; pick **`full`** → **Launch**.
11. **Stay on the Run** and watch the **preparation** phase: the Run announces that its sandbox is
    being prepared and **no node starts while it lasts** (tens of seconds on a real `~/.claude`).
    When preparation clears, the node starts, its terminal preview shows a live `claude` session
    that is **not** sitting on an interactive dialog, and the Run reaches **Completed** with its
    artifact readable from the host UI.
12. **Control twin.** Relaunch the same pipeline with sandbox **`off`**: no preparation phase, the
    node starts straight away, the Run reaches **Completed**. Compare the two Runs — same business
    outcome, visibly different route.
13. Open **Settings** → the sandbox section shows the **resolved Dockerfile** path with its tier and
    the **image tag** derived from it, and the **staging profile** editor lists the floor entry by
    entry.

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

#### Sandbox A/B (steps 10-13)

- The sandbox field lists `off`, `minimal`, `full` **and** any named staging profile; `full` and
  `minimal` are selectable with no prior configuration (they are virtual defaults).
- While the `full` Run prepares, **no node is running**. A node running while preparation is still
  announced is a **blocking finding** — that inversion is the #445 regression.
- The sandboxed node's terminal preview shows a live `claude` session **with no interactive dialog**:
  no managed-settings approval, no bypass-permissions warning. That silence is the entire point of
  the staging floor (#426) and it is only observable here.
- **Both** Runs reach **Completed**, End `result` **received**, and the output artifact opens from
  the host UI in both cases (for the sandboxed one, that is the merge-back).
- The `off` twin shows **no** preparation phase.
- Settings: the resolved Dockerfile path carries its tier, the image tag derives from it
  (`pdo-sandbox:h-<hash>`), the profile editor lists the floor entry by entry, warns on
  credential-bearing entries, and lists a profile's referents before confirming its deletion.

### Backing store

- A tmux session named for the run/node/iter is alive while the node runs and shows `claude` (not a
  bare shell); the output artifact file exists on disk under the run worktree.
- The Run's projected state and stats agree with daemon ground truth (run endpoint: `sessions_spawned`,
  `started_at`/`completed_at`, `loc`) and with git (`diff --numstat`, `.pdo/` excluded).

Sandbox A/B, read-only probes:

- While the sandboxed node runs, a **container named for the Run** exists, and that node's session
  really executes **inside** it (its session tail enters the container; from inside, the node is
  demonstrably not on the host).
- The daemon URL handed to the sandboxed session points at the **host gateway**, not `localhost` —
  the `off` twin's points at `localhost` (#447). The sandboxed manager can therefore actually reach
  the daemon it is told to command.
- The staged home of the `full` Run carries the **floor**: credentials, the org managed-settings
  baseline when the host has one, and a settings file bearing the bypass-permissions key.
- The `off` twin creates **no** container and **no** staging directory.

## Cleanup (best-effort)

- Archive the Run (`cleanup_run`): it reaps sessions and the worktree. Delete any pipeline the agent
  seeded.
- Archive **both** sandbox twins, and **assert** the cleanup rather than merely performing it: no
  container named for either Run survives, and the `full` Run's staging directory is gone. Its ~1 GB
  is reclaimed **only** here — a missed purge is the known disk-fill recurrence, and a silent leak
  is a finding.

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

### Sandbox A/B — why it is built this way

- **`full`, not `minimal`, and that is deliberate.** The ordering guarantee (no node before the
  sandbox is ready) can only regress when preparation is slow enough for the pipeline watcher to win
  the race. `minimal` prepares in under a second and wins by default, so it would make the defect
  invisible; `full` copies ~1 GB and takes tens of seconds, which is what exercises the guard. The
  cost is paid on purpose.
- **Stay on the Run during preparation.** This is load-bearing, not incidental: *reading* the Run's
  pipeline file is what wakes the watcher (an inotify `OPEN`, not a write), and that wake is the
  trigger that exposed #445. A drive-by that launches the Run and looks away is structurally blind to
  that class of defect — exactly how it survived a full slice validation.
- **Tens of seconds of preparation is not a stall.** Sandboxed Runs get a 15-minute grace in the
  stall detector. Only a Run still preparing past that grace is a finding.
- On a machine that does not yet have the sandbox image, the first sandboxed Run **builds** it
  (minutes). Expected once per image change, not a finding.
- **Do not assert a writable `$HOME` inside the container, nor a host uid ≠ 1000.** Both are known,
  filed gaps (#443, #414). Asserting them here would report a documented backlog item as a
  regression on every execution.
