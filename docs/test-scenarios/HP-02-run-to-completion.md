---
id: HP-02
covers: [run, start-node, tmux-session, dataflow, conditional-routing, loop-region, collection, merge, artifact, run-stats, sandbox, staging-profile, staging-floor, sandbox-prep, harness, harness-pin, harness-capability]
---

# HP-02 — Launch a run to completion

## Goal

A user launches a **Run** on a pipeline: picks a target repo, enters a prompt (optionally images),
and watches nodes spawn real **tmux sessions** running each node's resolved **harness** (`claude` is
the floor, not the only one), data route through edges / a loop region / a collection fan-out into a
**Merge**, until the Run reaches a clean **Completed** state with inspectable artifacts and live
stats — the core "drive an orchestration to its end" loop.

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
- **Agentic harness, as an A/B pair** (PRD #549, ADR-0045, ADR-0046): a **two-node** pipeline runs one
  node pinned to **`claude`** and one pinned to **`opencode`** in the **same Run**. The pair is the
  control, exactly as for the sandbox: a single-harness Run that silently resolved to the `claude`
  floor is indistinguishable from a correctly resolved one, so the second harness is what makes the
  four-tier resolution observable at all. See the journey's §14-15.

## Preconditions

- The app is running locally and reachable in a browser; status bar shows the daemon **connected**.
- `claude` is on `PATH` (the daemon shells out to it for each node session).
- `opencode` is on `PATH` too: the second harness of the embedded floor (ADR-0045), and the harness
  A/B needs both binaries resident.
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
    output and then completes. Open **New Run** on it → the sandbox field sits on **Use instance
    default**, and offers `off`, `minimal`, `full` and any named **staging profile**; pick **`full`**
    → **Launch**.
11. **Stay on the Run** and watch the **preparation** phase: the Run announces that its sandbox is
    being prepared and **no node starts while it lasts** (tens of seconds on a real `~/.claude`).
    When preparation clears, the node starts, its terminal preview shows a live `claude` session
    that is **not** sitting on an interactive dialog, and the Run reaches **Completed** with its
    artifact readable from the host UI.
12. **Control twin.** Relaunch the same pipeline with sandbox **`off`**: no preparation phase, the
    node starts straight away, the Run reaches **Completed**. Compare the two Runs — same business
    outcome, visibly different route.
13. Open **Settings** → on the sandbox side there is **only** `Default sandbox` and
    **Manage staging profiles…** (#471: the image source and Dockerfile fields moved to the profile).
    Open the profile editor → it lists the floor entry by entry, and its **Image** control offers
    `default` / `dockerfile` / `registry`, the `default` option saying in one sentence that the tag is
    the SHA-256 of the seeded Dockerfile's bytes.
14. **Harness A/B.** Seed a **two-node** pipeline: two parallel nodes, each asked for a single line of
    output. In the **node inspector**, pin the first node's harness to **`claude`** and the second's to
    **`opencode`**; each node's inspector reads back which harness it **resolves** to. On the
    `opencode` node, also set a **model** through the picker's `Custom…` escape hatch — a
    `provider/model` slug that supports tool use (e.g. `openrouter/anthropic/claude-haiku-4.5`).
    **This is not optional**, and the notes say why.
15. Open **New Run** on it. Set the Run's **Harness** field to **`claude`** and sandbox to **`off`**,
    then Launch. Setting the Run tier to `claude` is what turns the second node's pin into a real
    proof: it must still run `opencode` *against* the tier above it. Both nodes start, both reach
    **completed**, and the Run reaches **Completed** with the End `result` port **received**: one Run,
    two harnesses, one outcome.

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
- The field **leads with "Use instance default"** and that is where a freshly opened dialog sits.
  Set `default_sandbox` to a profile in **Settings**, **reopen** New Run *without reloading the page*
  (the reopen is the part that used to fail), launch **without touching the field**, and read the
  **`POST /runs` body**: it must carry **no** `sandbox` key. A `"sandbox":"off"` there is the #452
  regression — `off` is final for the daemon, so it makes `default_sandbox` unreachable, and nothing
  in the UI shows it. Only the request body does.
- While the `full` Run prepares, **no node is running**. A node running while preparation is still
  announced is a **blocking finding** — that inversion is the #445 regression.
- The sandboxed node's terminal preview shows a live `claude` session **with no interactive dialog**:
  no managed-settings approval, no bypass-permissions warning. That silence is the entire point of
  the staging floor (#426) and it is only observable here.
- **Both** Runs reach **Completed**, End `result` **received**, and the output artifact opens from
  the host UI in both cases (for the sandboxed one, that is the merge-back).
- The `off` twin shows **no** preparation phase.
- Settings: the sandbox side shows `Default sandbox` and the profiles button and **nothing else**
  (no image-source select, no Dockerfile input, no image tag — #471); the profile editor lists the
  floor entry by entry, offers the three-way **Image** control, warns on credential-bearing entries,
  and lists a profile's referents before confirming its deletion.

#### Harness A/B (steps 14-15)

- The node inspector offers **Default / claude / opencode** and reads back the **resolved** harness,
  saying whether that is a **pin** or the **floor** ("Resolved: opencode (pinned)" vs "Resolved:
  claude (floor — no pin)").
- Saving writes the pin as the node's own `pin_harness`, and the custom model under the **resolved
  harness's key** (`harnesses: { opencode: { model: … } }`), not as a flat `model:` — the per-harness
  map of #550, since a model means nothing outside a harness.
- The **effort picker is greyed on the `opencode` node** and live on the `claude` one. That is the
  descriptor's missing `{effort}` hole surfacing on screen (ADR-0045): an absence *declared*, not a
  defect, and the cheapest visible proof that the resolved harness reached the UI at all.
- **Est. cost reads "—" and names `opencode`** — never `$0`, and never the `claude` node's dollars on
  their own. A Run that launched a node on a harness with no cost source is not honestly summable, so
  the whole amount goes (#553, the `unpriced_models` vein of #425). A number reappearing there is the
  regression, and it is the one that would otherwise pass for a plausible total.
- Neither pane sits on an interactive dialog: `--auto` is `opencode`'s bypass flag, so a permission
  prompt on that pane is a finding.

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

Harness A/B, read-only probes:

- The `claude` node's session really runs `claude`, the `opencode` node's really runs `opencode`, **and
  each agrees with the harness frozen in that node's start event** (#550). The event is the contract —
  the harness is resolved once, at spawn, and never re-read from the YAML — so a pane that contradicts
  it is the finding, and this is the only place the disagreement is visible.
- Each pane's **real argv** matches its descriptor's template: the `claude` one carries
  `--dangerously-skip-permissions` and a `--session-id`, the `opencode` one carries `--auto --prompt`
  and **neither** `--session-id` (it cannot pin an identity) **nor** `--settings` (its template has no
  such hole). Those two absences are the descriptor's shape showing through to the process table.
- Both output artifacts carry the expected line. A `completed` status is not evidence on its own
  (#490): read the artifact.
- **Do not assert the `opencode` node's model.** Measured on 1.18.18: an unreachable model id falls
  back **silently** to another provider and the turn still goes green, so a model assertion there is a
  false pass either way. Same for any transcript-based probe: `opencode` cannot pin a session identity,
  so PDO attributes by working directory alone.

## Cleanup (best-effort)

- Archive the Run (`cleanup_run`): it reaps sessions and the worktree. Delete any pipeline the agent
  seeded.
- Archive **both** sandbox twins, and **assert** the cleanup rather than merely performing it: no
  container named for either Run survives, and the `full` Run's staging directory is gone. Its ~1 GB
  is reclaimed **only** here — a missed purge is the known disk-fill recurrence, and a silent leak
  is a finding.
- Archive the **harness A/B** Run and delete its two-node pipeline. Leave `opencode`'s own store
  (`~/.local/share/opencode/`) **alone**: the run legitimately writes a session there, that is the
  harness's business, and deleting a user's harness database is not cleanup.

## Notes

- **The clean-terminal-state check here is the happy ending — not adversity.** Deep failure modes
  (daemon kill, session death, admission-slot leak, mid-run-edit rejection) are edge cases covered by
  the layer-3 automated tests, **not** by this HP.
- **Select data by characteristics, not hard-coded ids** (HP mode): if no data satisfies a condition,
  that is a legitimate finding, not an excuse to bypass the UI.
- A first `claude` launch in a fresh worktree lands on the trust dialog — confirm it (see the driving
  playbook) before expecting chat output. That one is **`claude`'s**, not the product's: `opencode
  --auto` shows no such dialog, so a dialog on an `opencode` pane is a finding rather than a step.
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
- **Do not assert a writable `$HOME` inside the container.** That is a known, filed gap (#443).
  Asserting it here would report a documented backlog item as a regression on every execution. The
  host-uid half of this note is **gone**: since #414 a named identity is injected into the container,
  so `whoami` and `sudo -n true` DO work under any host uid — a failure there is a finding.

### Harness A/B — why it is built this way

- **Its own two-node pipeline, not a pin on the main journey's nodes.** Those nodes carry the dataflow
  (conditional routing, loop region, collection, merge); a node that fails to complete among them costs
  six steps of assertions downstream. Isolating the newest axis is the same call the sandbox twin makes
  by getting its own one-node pipeline.
- **The two axes are not crossed, on purpose.** HP-02 already pays ~1 GB of staging for the sandbox
  pair; harness × sandbox would be four combinations for no extra information. The harness pair rides
  the `off` side, where it costs two panes.
- **`opencode` completes because the agent runs `pdo complete` itself, and for now that IS the
  contract.** It has neither of PDO's automatic substrates: no turn-end `Stop` hook (its argv template
  carries no `{settings}` hole, so the hook file is written and never referenced) and no turn-end sweep
  probe (no `turn_end_substrate` capability, #553). That instrumentation is deliberately later work and
  **is not a defect to raise here** — what this journey asserts is the outcome, that the node reaches
  completed.
- **Bound the wait on the `opencode` node.** Because the harness is *resident* (ADR-0045: it stays
  alive after its turn), a node that never self-completes stays `running`, **alive and mute rather than
  dead** — there is no session death to notice and nothing times it out. Give it a finite wait and
  raise a finding on the harness's obedience; do not sit on it, and do not read the silence as the
  missing auto-completion above.
- **The `opencode` node MUST carry a model, and this is the trap that sinks a first run.** PDO passes
  no `--model` when a node declares none, and `opencode` then resolves its **own** default from the
  operator's config — whatever they last selected. Measured here: it landed on an *image* model and the
  turn died on `No endpoints found that support tool use`, so the node could neither write its artifact
  nor complete, and simply sat there resident. Nothing in PDO is at fault and nothing warns: a model id
  is meaningless outside its harness, so `opencode` needs an explicit `provider/model`.
- **The model picker still speaks `claude` on an `opencode` node.** It offers `sonnet` / `opus` /
  `haiku` / `opusplan` / `fable` — Anthropic aliases that mean nothing to `opencode` — so `Custom…` is
  the only usable path. The effort picker greys correctly off the descriptor; the model *list* does not
  follow the resolved harness. Do not pick `haiku` here and assume it took.
- **`opencode`'s residency is conditional, and its exit reads as a session death.** It is resident
  after a *completed* turn (that is its ADR-0045 eligibility), but after the hard provider error above
  it exited on its own a couple of minutes later, which surfaced as `session_died` and reconciled the
  Run to `failed` with `run_stalled: … blocked behind: <node>`. Two things it is **not**: the PTY
  bridge (it kills its own `tmux attach` client on socket close, never the session — verified by
  opening and closing a bridge on a live session, which survived), and memory pressure (the detector's
  own diagnostics carry `mem_available_kb` / `swap_free_kb`, so read them before blaming the machine).
