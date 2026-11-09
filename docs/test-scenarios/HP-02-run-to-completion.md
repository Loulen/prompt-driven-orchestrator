---
id: HP-02
covers: [run, project, start-node, tmux-session, dataflow, conditional-routing, loop-region, collection, convergence, artifact, run-stats, approve, sandbox, staging-profile, staging-floor, sandbox-prep, harness, agent-choice, harness-capability, harness-turn-end, harness-reported-cost, harness-resume-by-identity]
---

# HP-02 — Launch a run to completion

## Goal

A user launches a **Run** on a pipeline: picks a target repository, enters a prompt (optionally
files), and watches nodes spawn real **tmux sessions** running each node's resolved **harness**
(`claude` is the floor, not the only one), data route through edges / a loop region / a collection
fan-out that **converges on an agent** (ADR-0082: there is no Merge node — any node fed by several
edges is a convergence), until the Run reaches a clean **Completed** state with inspectable
artifacts and live stats — the core "drive an orchestration to its end" loop, on the core alone
(no extension).

## Drive-by

Features validated while crossing the run screens:

- **Repo explorer** (#131): the magnifier of the Repo field (« Browse folders ») opens a folder
  explorer (folders only, dotfiles hidden, git and symlink marks) that picks a folder through the
  ordinary validation path (« Select this folder ») and degrades gracefully on an unreadable one.
- **Project row** (ADR-0087): New Run leads with a « Project » row (or « No project yet » and
  « New project »); a Run launched in no Project still runs.
- **Run files** (#145): files attached in New Run's « Files » field show in the **Start inspector**
  (images as thumbnails, with the lightbox). The Start card itself shows none (CORE-222).
- **Conditional routing** (#144): a producer fans out to **all** matching guarded edges
  (multi-match); an `else` edge catches the unmatched case; pills are always visible.
- **Loop region** (#148): a bounded review loop exits early on its PASS edge; its corner reads the
  lap `i/N` and the loop's cost (`· ~$x`). A verdict that never passes parks the Run on the region,
  which reads « exhausted · unrouted » with « Route from manager » — never a silent stall.
- **Collection fan-out / convergence** (#151, ADR-0026, ADR-0082): a `kind: collection` region fans
  its single-member body out in parallel — one lap per item, the corner reading `⇉ X/N items` —
  the barrier fires once when all laps finish, and an empty collection fires it immediately. The
  node downstream is an ordinary **agent**: it starts only once every lap is done, receives the
  laps' outputs grouped in one input, and finds each lap's code already merged back on the Run
  branch. The editor toolbar offers no Merge button.
- **Approve ▾** (ADR-0068): on a node with no output yet, Approve is inert, its tooltip
  « Missing outputs: <ports> »; the daemon answers **409 `missing_outputs`** to a forced completion.
- **Artifact rendering** (#240 / ADR-0013): an output opens in the markdown modal; a ` ```mermaid `
  block renders as inline SVG and invalid mermaid degrades to its raw source.
- **Run stats** (#100 / #272): the **Info** tab's « Stats » strip shows Duration (ticking live),
  Node sessions started (manager excluded; **hidden by default**: turn on Settings › Appearance ›
  Run info › Stats › « Node sessions » first), Changes (`+N −M`, a click opens the Diff tab) and the
  **Est. cost** (« — » when it cannot be computed); every label is the cell's tooltip.
- **Runs grouped by Project** (#258, ADR-0087): the Runs list groups by Project, falling back to
  the repository for Runs outside any Project, as soon as there are two groups or one Project group.
- **Daemon version** in the status bar (#139): `v<version>`, live.
- **Sandbox, as an A/B pair** (PRD #403, ADR-0030, ADR-0031): the same one-node pipeline is launched
  twice — once on the staging profile **`full`**, once **Off, on the host** — and the two Runs must
  reach the same outcome by two visibly different routes. The `off` twin is the **control**: a green
  sandboxed Run proves nothing alone (a Run that silently fell back to the host path also looks
  green). See steps 10-13.
- **Agentic harness, as a four-way choice** (PRD #549 / #612 / #702, ADR-0045, ADR-0046, ADR-0051,
  ADR-0052, ADR-0063): a **four-node** pipeline runs one node on **`claude`**, one on **`opencode`**,
  one on **`copilot`** and one on **`pi`** in the **same Run**. The set is the control: a
  single-harness Run that silently resolved to the `claude` floor is indistinguishable from a
  correctly resolved one. The four are also the whole spread of PDO's instrumentation in one Run —
  what the **support table** of `docs/reference/harnesses.md` publishes. See steps 14-16.

## Preconditions

- The app (the core alone, no extension) is running locally and reachable in a browser; the status
  bar shows « Daemon: connected ». See `docs/agents/run-scenario.md` to start a demo daemon and
  probe side-effects; no hard-coded ports, ids or paths in the journey.
- `claude`, `opencode`, `copilot` and `pi` are on the **daemon's** `PATH` (enriched from your login
  shell, ADR-0055) and **logged in** on this machine: `opencode` needs its
  `~/.local/share/opencode/auth.json` (a provider login, `opencode auth login`), or its node dies
  at once with `ProviderModelNotFoundError`. The target repository's root has been **trusted
  once** for `copilot` (`docs/reference/harnesses.md` § Prerequisites): the trust dialog is not
  covered by its autonomy flags, and trust cascades to every node sub-worktree. The demo daemon
  stages `copilot`'s logged-in user and trusts `/tmp` for it: keep the target repo under `/tmp`.
- Docker is available for the sandbox pair; the sandbox image may be built on the first sandboxed
  Run (minutes, once per image change). The daemon listens on an address the container reaches:
  start the demo daemon with **`BIND_ALL=1`** (`docs/agents/run-scenario.md`). A loopback-only
  daemon refuses the container's `host.docker.internal` calls, and the sandboxed node can never
  complete.
- A target git repository, and the pipelines the journey seeds (a dataflow pipeline with a
  conditional edge, a bounded review loop and a collection region converging on an agent; a
  one-node pipeline; a four-node pipeline). A pipeline created for the journey is born a **test
  pipeline**: unfold « Show test pipelines » in New Run's pipeline menu, or Validate it first.

## Journey

1. Open **New Run** (« New run » at the top of the Runs tab) → **pick a repo**: the magnifier opens the folder explorer;
   select a git repository → the Repo field shows its check (« Valid git repository ») and a
   « Branch » row appears.
2. Choose the dataflow pipeline, **enter a prompt** (« Prompt »), attach an image in « Files » →
   **Run**; capture the resulting Run.
3. The canvas shows the **Start node** (▶, tooltip « Run start ») and the **End node** (■,
   tooltip « Run end ») with the dataflow between them; within a couple of seconds the first work node animates to
   **running**. Select Start → its inspector shows the prompt and the image.
4. Select the running node → the right panel shows a **live terminal** (« attached · live », a real
   `claude` session, wrapping without horizontal scroll) and, in its **I/O** tab, the folded
   « Prompt » section holding the prompt preamble (`## Inputs` / `## Outputs`). Approve ▾ is inert
   until the node has written its outputs.
5. Data routes downstream: conditional edges fire to all matching targets, the loop region iterates
   (`i/N` in its corner) and exits on PASS, the collection region fans out in parallel (`⇉ X/N
   items`) and converges on the downstream **agent**.
6. The Run reaches **Completed**: nodes read completed, End carries its « Reached » check (tooltip
   « Run end · reached »), and its inspector's « Termination reasons » show the `result` port
   **received**.
7. Open an output → the **markdown modal** renders it (a mermaid diagram as SVG).
8. Open the Run's **Info** tab → the **Stats** strip shows Duration (frozen now), Node sessions
   started (switched on beforehand, see the description), Changes and Est. cost.
9. Find the Run in the **Runs list** (grouped by Project, or by repository outside a Project, when
   there are two groups or one Project group).
10. **Sandboxed twin.** Seed (or reuse) a **one-node pipeline** whose node writes a single line and
    completes. Open **New Run** on it → the « Sandbox » field sits on « Default (<instance
    default>) » and offers « Off, on the host », `minimal`, `full` and any named **staging
    profile**; pick **`full`** → **Run**.
11. **Stay on the Run** and watch the **preparation**: its Info tab shows « Preparing the sandbox »
    (pulling or building the image) and **no node starts while it lasts** (tens of seconds on a
    real `~/.claude`). When it clears, the node starts, its terminal shows a live `claude` session
    that is **not** sitting on an interactive dialog, and the Run reaches **Completed** with its
    output readable from the host UI.
12. **Control twin.** Relaunch the same pipeline on « Off, on the host »: no preparation, the node
    starts straight away, the Run reaches **Completed**. Same outcome, visibly different route.
13. Open **Settings › Sandbox & worktrees** → « Default sandbox » holds **only** the default sandbox
    select (#471). « Staging profiles » is inline (#691: no drawer, no dialog; its header carries the
    « Saved at once » mark) and lists each profile's entries by group (« Floor », « From the
    default », « Extras »), flags the credential-bearing ones (« Stages secrets »), and its
    **Image** control offers `default` / `dockerfile` / `registry`, `default` showing
    `~/.pdo/sandbox/Dockerfile` (its tooltip: the tag is the SHA-256 of that file's bytes).
14. **Four-way harness choice.** Seed a **four-node** pipeline: four parallel nodes, each asked for
    a single line of output. In each node's inspector, on the « Model » row, set the **Agent**
    switch (Inherit / Profile / Custom) to **Custom** and pick its **Harness**: `claude`, `opencode`, `copilot`, `pi`. Picking a harness fills its own model and
    effort: on the `opencode` node, type a `provider/model` slug that supports tool use (e.g.
    `openrouter/anthropic/claude-haiku-4.5`) in its free-text model field — **this is not
    optional**, see the notes; on the `copilot` node, read the Model and Effort lists, then set the
    Model back to `default`. Save.
15. Open **New Run** on it. In « Agent », choose « Custom… », Harness **`claude`**, Apply; Sandbox
    « Off, on the host » → **Run**. The Run tier on `claude` is what makes the other three nodes a
    proof: they must still run `opencode`, `copilot` and `pi` against the tier above them. All four
    nodes reach **completed** and the Run **Completed**, End's `result` **received**: one Run, four
    harnesses, one outcome.
16. **The `copilot` node, end to end.** Watch it without touching it: it starts, its terminal shows
    an **interactive** `copilot` session (not a one-shot that exits), and when its turn ends the
    node goes **completed on its own**: nobody pressed Approve, nobody typed in its pane. The agent
    may run `pdo complete` itself (its preamble asks every agent to); when it doesn't, the runtime
    completes the node on turn end. Either way the node must not stay running. Then
    open the Run's **Info** tab: Est. cost reads « — » followed by one slice per costed harness, in
    alphabetical order — `— · ~$… via claude · ~$… via copilot · $… via pi` (`~` marks an estimate;
    `pi` reports exact dollars) — its tooltip naming `opencode` as the harness with no cost
    source. Finally restore the finished
    node's folded terminal bar (« Show the terminal ») → the inset shows the snapshot PDO froze on
    the way out, labelled `snapshot · session reaped`, with no detach button: **this node's own
    conversation** — its prompt, its turn, its `❯` prompt back and waiting.

## Checks

### Surface

- Start/End render; the running node shows live, wrapping terminal output (no horizontal scrollbar).
- Routing matches the pipeline shape (multi-match fan-out, the loop corner iterating, the
  collection's `⇉ X/N items`, the convergence drawn as an agent, not a Merge).
- The Run settles to **Completed**; End's `result` reads **received**.
- The artifact modal shows the content; a valid mermaid block is an SVG, an invalid one falls back
  to `<pre><code>` (never a blank pane, never a thrown error).
- Stats: Duration ticks on a live Run and freezes on a terminal one; Est. cost is labelled an
  estimate, « — » when it cannot be computed.
- The browser console shows no uncaught error along the journey.

#### Sandbox A/B (steps 10-13)

- The Sandbox field lists « Off, on the host », `minimal`, `full` **and** any named staging
  profile; `full` and `minimal` are selectable with no prior configuration (built-in profiles).
- The field **leads with « Default (<instance default>) »** and a freshly opened New Run sits on
  it. Set Default sandbox to a profile in **Settings** and **Save** the form, **reopen** New Run
  *without reloading the page*, launch **without touching the field**, and read the **`POST /runs`
  body**: it carries **no** `sandbox` key. A `"sandbox":"off"` there is the #452 regression — `off`
  is final for the daemon, so it makes the default unreachable, and only the request body shows it.
- While the `full` Run prepares, **no node is running**. A node running while « Preparing the
  sandbox » still shows is a **blocking finding** — that inversion is the #445 regression.
- The sandboxed node's terminal shows a live `claude` session **with no interactive dialog**: no
  managed-settings approval, no bypass-permissions warning (#426, ADR-0063).
- **Both** Runs reach **Completed**, End's `result` **received**, and the output opens from the host
  UI in both cases (for the sandboxed one, that is the merge-back).
- The `off` twin shows **no** preparation.
- Settings: the Default sandbox section holds the select and **nothing else** (no image-source
  select, no Dockerfile input, no image tag — #471); deleting a profile lists its referents before
  confirming.

#### Harness four-way choice (steps 14-16)

- The node inspector's « Model » row holds the **Agent** switch, Inherit / Profile / Custom; Custom's **Harness** lists
  `claude`, `opencode`, `copilot`, `pi` (a missing binary reads « not installed »). In Inherit or
  Profile, a line under it reads the resolved `harness · model · effort`.
- Saving writes each node's `agent_choice: {mode: custom, harness: …, model: …, effort: …}` in the
  YAML — the model under the harness it belongs to, never a flat `model:` alone.
- The **Effort** select shows on `claude` with `claude`'s stops (`low · medium · high · xhigh ·
  max`) and its Model offers the aliases the binary names (`fable`, `opus`, `sonnet`); it is
  **absent** on `opencode` (no effort hole in its descriptor, ADR-0045 — an absence *declared*, not
  a defect). A `claude` picker that offers only « default » is a **finding**: the catalogue reader
  went blind on `claude`'s help.
- On `copilot`, the Effort offers **`copilot`'s own** stops (`none · minimal · low · medium · high ·
  xhigh · max`, ADR-0053) and the Model a list read from the binary (its completion script first, ADR-0056). The effort
  picked there reaches the pane's argv as `--effort <level>`.
  Anthropic's five stops or `claude`'s aliases there mean the `claude` catalogue leaked onto another
  harness — a finding. On `pi`, the Effort offers pi's `--thinking` stops.
- **Est. cost reads « — » and its tooltip names `opencode`** — never `$0`, never the others' dollars
  passed off as the Run's total (#553). Beside the « — », the cost is **ventilated by harness**:
  `via claude` (derived from tokens), `via copilot` (reported by the harness and converted by a
  constant, ADR-0052) and `via pi` (dollars reported by the harness, #707) — said apart, never
  summed. A bare « — » with no breakdown is
  the regression (#617); a `copilot` slice that went through the price table is the other one.
- No terminal sits on an interactive dialog: `--auto` is `opencode`'s bypass flag and `--allow-all
  --no-ask-user` is `copilot`'s, so a permission prompt on either is a finding. A **trust dialog**
  on the `copilot` pane is the unmet prerequisite, not a product finding: trust the repo root and
  rerun.
- **A finished node's terminal shows its frozen pane.** Restoring the folded bar renders the
  snapshot, says it is one, offers no detach. Raw tmux text (`can't find session: pdo-…`) under a
  `disconnected` badge is a **finding** (#617).
- **`copilot` and `pi` complete without anyone completing them** (`copilot`'s journal
  `assistant.turn_end`, ADR-0051; `pi`'s `agent_settled` extension). One still `running` after its
  pane has visibly finished is a finding.
- What the four nodes did matches what the **support table** of `docs/reference/harnesses.md` says
  of each harness (cost, transcript, end of turn) — the one place the published table and the
  running product are compared.

### Internals

- A tmux session named for the run/node/iter is alive while the node runs and shows the harness
  (not a bare shell); the output artifact exists on disk under the Run's worktree.
- The Run's projected state and stats agree with the daemon (`GET /runs/<id>`: `sessions_spawned`,
  `started_at` / `completed_at`, `loc`) and with git (`diff --numstat`, `.pdo/` excluded).

Sandbox A/B, read-only probes:

- While the sandboxed node runs, a **container named for the Run** exists, and that node's session
  really executes **inside** it.
- The daemon URL handed to the sandboxed session points at the **host gateway**
  (`host.docker.internal`), the `off` twin's at `localhost` (#447).
- The staged home of the `full` Run carries the **floor**: credentials, the org managed-settings
  baseline when the host has one, and a settings file bearing the bypass-permissions key.
- The `off` twin creates **no** container and **no** staging directory.

Harness four-way choice, read-only probes:

- Each node's session runs the binary it was given, **and agrees with the harness frozen in that
  node's start event** (#550). The event is the contract — the harness is resolved once, at spawn,
  never re-read from the YAML.
- Each pane's **real argv** matches its descriptor: `claude` carries
  `--dangerously-skip-permissions` and a `--session-id`; `opencode` carries `--auto` and `--prompt` (its
  `--model` in between) and **neither** `--session-id` **nor** `--settings`; `copilot` carries
  `--allow-all --no-ask-user`, a `--session-id`, `--effort` when one is set, and **`-i`** before the
  prompt (`-p` would be a one-shot, ineligible under
  ADR-0032; a positional prompt is refused by the binary, #615).
- **Resume by identity.** The `--session-id` on the `copilot` pane equals the id in that node's start
  event, and its journal sits at `<copilot store>/<that id>/events.jsonl` — keyed by the identity
  PDO imposed, not by the working directory.
- **A finished node serves a snapshot, not a live session.** `GET /runs/<id>/nodes/<node>/pane`
  answers `source: "snapshot"` with the conversation that ran there; the tmux session is **gone**
  (reaped on the terminal transition, every harness). A live session outliving a terminal node, or
  an empty pane, is a finding.
- The `copilot` node's completion comes from the node itself: the agent's own `pdo complete`, or
  the runtime's on turn end (`assistant.turn_end` in its journal) when the agent didn't run it. Both
  are fine; which one happened depends on the model, so don't assert it. A journal ending in
  `session.error` must **not** have completed the node (`copilot` exits 0 on a hard model failure,
  ADR-0052).
- All four outputs carry the expected line. A `completed` status alone is not evidence (#490):
  read the artifact.
- **Do not assert the `opencode` node's model** (see the notes).

## Cleanup (best-effort)

- Archive every Run (⋯ › Cleanup, or `cleanup_run`): it reaps sessions and worktrees. Delete every
  pipeline the agent seeded.
- For the sandbox twins, **assert** the cleanup: no container named for either Run survives, and the
  `full` Run's staging directory is gone. Its ~1 GB is reclaimed **only** here — a silent leak is a
  finding.
- Leave `opencode`'s store (`~/.local/share/opencode/`), `copilot`'s and `pi`'s session stores, and
  the repo's `copilot` trust approval **alone**: they are the harnesses' business and a
  prerequisite, not Run residue.

## Notes

- **The clean terminal state is the happy ending — not adversity.** Daemon kill, session death,
  admission-slot leak, mid-run edit rejection are covered by the layer-3 automated tests.
- **Select data by characteristics, not hard-coded ids**: if no data satisfies a condition, that is
  a finding, not an excuse to bypass the UI.
- A `claude` launch in a worktree its config doesn't trust lands on its trust dialog. The demo
  daemon trusts `/tmp`; if the dialog shows anyway, accept it with **Down, then Enter**: its cursor
  starts on « ❯ No, exit », and a bare Enter ends the session (see the driving playbook). That
  dialog is `claude`'s, not the product's; `copilot`'s is a prerequisite, set once before the Run.
- Nothing in the journey names the product or shows its logo: the brand shown in the window is out
  of scope here.

### Sandbox A/B — why it is built this way

- **`full`, not `minimal`, on purpose.** The ordering guarantee (no node before the sandbox is
  ready) only regresses when preparation is slow enough for the pipeline watcher to win the race;
  `minimal` prepares in under a second and would hide the defect.
- **Stay on the Run during preparation.** *Reading* the Run's pipeline file wakes the watcher (an
  inotify `OPEN`), and that wake is what exposed #445. A journey that launches and looks away is
  blind to that class of defect.
- **Tens of seconds of preparation is not a stall.** Sandboxed Runs get a 15-minute grace in the
  stall detector; only a Run still preparing past it is a finding.
- **Do not assert a writable `$HOME` inside the container** (a filed gap, #443). Since #414 a named
  identity is injected, so `whoami` and `sudo -n true` work under any host uid — a failure there
  is a finding.

### Harness four-way choice — why it is built this way

- **Its own four-node pipeline**, not a choice on the dataflow nodes: a node that fails among those
  costs every assertion downstream.
- **The two axes are not crossed.** HP-02 already pays ~1 GB of staging for the sandbox pair; the
  harness set rides the `off` side, where it costs four panes.
- **The `opencode` node MUST carry a model.** With none, `opencode` resolves its **own** default from
  the operator's config; it once landed on an image model and the turn died on `No endpoints found
  that support tool use`, leaving the node alive and mute. A model id means nothing outside its
  harness, so `opencode` needs an explicit `provider/model`.
- **`opencode` completes because the agent runs `pdo complete` itself**: it has no end-of-turn
  substrate (support table, « End of turn »). That is declared, **not a defect to raise** — the
  journey asserts the node reaches completed. **Bound the wait**: a resident harness that never
  completes stays `running`, alive and mute; raise a finding on the harness's obedience after a
  finite wait.
- **Do not assert the `opencode` model**: an unreachable model id falls back **silently** to
  another provider and the turn still goes green, so a model assertion is a false pass either way.
  `opencode` pins no session identity, so any transcript probe on it is by working directory only.
- **`opencode` can exit on its own after a hard provider error**, which surfaces as `session_died`
  and a `failed` Run (`run_stalled: … blocked behind: <node>`). It is neither the PTY bridge nor
  memory pressure (the detector's diagnostics carry `mem_available_kb` / `swap_free_kb`).
- **The `copilot` node needs no model**: an unset model drops the hole and `copilot` picks its own
  — which is why step 14 sets it back to `default` after reading the list.
- **`copilot`'s exit code is not a verdict** (ADR-0052): read the journal; a trailing
  `session.error` is the failure.
- **A `copilot` node mute at its first turn is almost always the trust dialog**, not PDO.
- **Do not kill a session to watch it resume.** Interrupt-and-recover is adversity, covered at
  layer 3; the resume-by-identity contract is asserted through the pinned `--session-id`, the
  journal path keyed by it, and the snapshot showing this node's own conversation.
- **Residency is read while the node runs, or off the snapshot afterwards — never by attaching to a
  finished node**: PDO reaps a node's session the moment it goes terminal (the one-live-iteration
  invariant).
