---
id: HP-03
covers: [tour, tour-step, step-replay, projecteur, training-repository, welcome-modal, run, interactive-node, completion-guard, artifact, script-node, trigger, tidy-up, pipeline-validation, tour-identity, status-icon]
---

# HP-03 — A newcomer takes the full tour

## Goal

Somebody opens PDO for the first time and is offered the **full tour**. Taking it, they first walk
around the screen on a real, finished **Run** of `script` nodes (*Overview*, the tour préface), then
they launch a real
**Run** on a **training repository**, talk to the agent in its own session, release the **completion
guard**, watch the node finish and read its output — then chain straight into building a pipeline of
their own. The journey is the product's front door: if it breaks, the first five minutes of every new
user break with it, silently.

This is also the only HP that exercises the **tutorial mode** end to end, and the only one that
asserts the two halves of the promise in ADR-0071: a tour drives the **real UI**, and it advances on
**observed state**, never on a delay.

## Drive-by

Features validated while crossing the tour:

- **Welcome modal rule** (#823): offered only on a browser with no « tutorial offered » key **and**
  an instance with no Run at all. Answering it — with any of its three choices — sets the key.
- **Projecteur** (#823): the window dims except the step's target, clicks outside are absorbed, the
  target underneath receives a normal click, and quitting is always one control away.
- **Training repository** (#824): a throwaway git repo created under `/tmp` by a daemon verb that
  knows nothing about tours, reused as-is when it already exists.
- **New Run form**, field by field: the filesystem explorer, the pipeline menu, the agent profile
  (`Default` over `Inherit`), the two PDO skills, the prompt.
- **Validated pipelines** (CONTEXT.md « Pipeline validé »): *First run* prepares a Projet
  « Tutorial » holding the training repository and validates `tutorial-interactive` for it alone, so
  the pipeline menu heads with « Project pipelines · Tutorial »; *First pipeline* ends by validating
  the pipeline it built (Validate, next to Save, and its modal), and the tab bar's indicator follows.
- **Interactive node** and its **completion guard** (ADR-0068): the node cannot end until a human
  releases it, and the release is observable in the inspector.
- **Artifact modal** on the node's `out` port — the markdown the agent wrote.
- **Settings › General › Tutorials** (#823): a checkmark per finished tour, in this browser.
- **Preparation and tidy-up** (#911, ADR-0071 §4): *Overview* prepares the `tutorial-overview`
  pipeline (two `script` nodes), an example Trigger whose guard always refuses, and a Run it waits
  for until `completed` — then removes the example Trigger when the tour ends.
- **Archiving a Run** (#911): the tour's Run is archived by the real gesture; its row shows the
  archive icon and its outputs stay readable.
- **Identité de tour** (ADR-0071 §5): this browser draws a four-character identity once
  (`[0-9a-z]`), and every object a tour prepares or has you paste carries it as a suffix — written
  `<id>` below.
- **Read-only panels** (#911): the panel a gesture opens (node inspector, edge panel, Start
  inspector) stays lit; its tabs switch, it scrolls and unfolds, but nothing in it can be edited
  while the tour shows it.
- **Step replay** (CONTEXT.md « Rejeu d'étape »): `‹ Previous` (or **←**) replays the step before
  as an ordinary step; **→** presses `Next` when the card shows it live. The arrows belong to a
  field, the terminal, the canvas or a list while one of them has the focus.

## Preconditions

- The app (the core alone, no extension) is running locally and reachable in a browser; the
  status bar shows « Daemon: connected ».
- A **harness is configured and on PATH** — *First run* launches a real Run with a real agent. Without
  one the tour stops on its refusal card, which is correct behaviour but not this journey.
- **A browser with no tutorial memory** (a fresh profile, or the tour keys cleared) and an instance
  **with no Run**. Both halves matter: the welcome modal is the entry point being tested.
- No hard-coded ports, ids or paths in the journey — see `docs/agents/run-scenario.md` for how to
  drive PDO.

## Journey

1. Open the app on a Run-less instance in a fresh browser → the **welcome modal** offers the full
   tour, each tour on its own, and « later ».
2. Choose the **full tour** → the *Overview* tour opens on its card, with a line per
   **preparation**: training repository, `tutorial-overview` pipeline, example Trigger
   `tutorial-example-trigger-<id>`, and a Run `tour-overview-<id>`.
   The card's header reads « ~4 min · 18 steps »; `Start` stays dead until the Run is `completed`
   (a few seconds: its nodes are scripts, no harness involved).
3. Walk *Overview*'s eighteen cards: open the tour's Run from the list, read the canvas and both
   panels (with nothing selected, the right pane shows the Run's **Pipeline info**; it is open, not
   folded away, because a Run tab always has something to show), click **implementer** (side bubbles name its `task` input and its Config), click its **`code`** output
   (the viewer opens and the card stops on it for `Next`), **close the viewer** (✕ or Escape — the
   card advances on the close), then unfold its folded **Terminal** (frozen on the script's lines;
   its expand button lifts it over the app as the **enlarged terminal**, still lit, and the same
   button puts it back),
   click the **reviewer → End** edge (a bubble points at `verdict eq pass`), click **Start** — the
   card says first that it shows the prompt the Run was started with, and a bubble points at that
   input once it is open — read the Runs tab (the card sends you back to it if you left it) and the
   **status icon** (the card explains the shapes and draws four Status icons: a check, a turning
   circle, a question mark, a cross; no colour is named; its footer says « Hover the status icon of
   your Run », and hovering it moves on), read *Each Run has its worktree*, **archive** the Run
   (its row's ⋯ › Cleanup, then confirm; the row shows the archive icon), open **Triggers** (the example Trigger is listed) and **Pipelines**
   (`tutorial-overview` is listed, without a reload), then Settings and Stats, lit but not opened.
   The gestures advance on the observed state — and stop for `Next` when the card has something new
   to say; the rest take `Next`. There is no `Previous` on the first card. **On *The left panel*,
   press → instead of clicking `Next`, then ← once** → the card before comes back, at the step
   number before, waiting for `Next`; → takes you forward again. (While the focus is still on the
   Run row you clicked, the arrows belong to the Run list, not to the tour: click the card's body
   first.) **On *Its terminal*, hover `‹ Previous`** → it is greyed out, and its tooltip says
   « This step's screen is gone: it can't be replayed »: the viewer *Close the output* asked to
   close is gone.
4. The **intermediate card** recaps *Overview* and offers **Continue** (next: *First run*). **Continue** → the
   example Trigger is gone from the Triggers tab, and the *First run* tour opens on its card, which
   names what it is about to do and shows a line per **preparation** (training repository, training
   pipeline, Projet « Tutorial » with the pipeline validated for it). `Start` stays dead until all
   three have answered.
5. `Start` → the first step lights the **New Run** button and nothing else — or, when the reader is
   not on the Runs tab (the full tour arrives from *Overview*, which ends on the Pipelines tab), the
   **Runs** tab first, then New Run. A click anywhere else does nothing at all.
6. Walk the form, doing exactly what each card asks: name the Run (untick « Auto », next to the
   Name field; the block to paste is `my-first-run-<id>`), pick the training repository
   through the **magnifier** (the explorer opens on `/tmp`, where the tour created it: click the
   `pdo-tutorial` row, then « Select this folder »), choose the tutorial
   pipeline in the **pipeline menu** — it heads the menu under « Project pipelines · Tutorial », before
   « Global pipelines » — pick the `Default` agent profile, tick **both** PDO skills in
   the skills picker — which stays open across the two ticks, the card's checklist ticking as you go,
   and folds back to « 2 active skills » — paste the prompt → each step advances **on the gesture**,
   with no extra confirmation but the ones the card asks for. A step whose condition the form already
   meets (New Run may open on the Projet « Tutorial », its repository and pipeline filled in) waits
   for `Next` instead: that is the tour reading the state, not a skipped step.
7. **Run** → the Run appears in the list on the left and the tour points at its row. The launch also
   opens the Run's tab by itself, so the card says so, **naming the Run** (« Your Run,
   my-first-run-<id>, is lit in the list on the left… », never « the top row »), and waits for a
   `Next` — the step is shown, never skipped past.
8. Open the **node** on the canvas → the inspector shows its live terminal. (When the node is
   already selected, the card says the inspector already shows it and waits for `Next`.)
9. **Talk to the agent**: the card hands over a line to copy; paste it into the terminal and press
   Enter. **Escape inside the terminal belongs to the agent** — it must not end the tour.
   ← / → inside the terminal belong to the agent too: they must not move the tour. Don't press
   them in the harness's prompt itself (Claude Code's ← on an empty prompt switches views).
10. Open the menu of **Approve ▾** and choose **Auto-approve when ready** → the tour advances on the released guard, not on the click.
11. The **wait** step lights the whole inspector as a soft zone — the terminal stays readable and
   usable — shows a two-item checklist (released / finished) and says it has **no time limit**. It
   ends by itself when the node's status becomes terminal. **Enlarge the terminal** while you wait
   (its expand button): the zone follows the enlarged terminal, with no dashed outline drawn across
   it.
12. **Open the output.** With the terminal still enlarged, the card first asks to put it back: it
    lights the terminal's **Collapse terminal** button (top right of its bar) and says the `out` row
    is under it. Click it → the card re-aims onto the **`out`** row under Outputs. Click the row →
    the artifact modal shows the summary the agent wrote, lit as a soft zone with the card beside
    it. Read it, then `Next`. Every target lit on the way is reachable with the mouse; Skip is never
    the only way on.
13. The **intermediate card** says how far along the full tour is, recaps what was actually observed,
    and offers *Finish here* or **Continue** (next: *First pipeline*).
14. **Continue** → the artifact modal closes and the *First pipeline* tour starts on a clear screen.
    Carry it to its end card and `Finish`; the name to paste is `tutorial-implement-test-<id>`.
    Right after **Save**, the card asks to **Validate the
    pipeline**: Validate (next to Save) opens the modal; tick **All projects (current and future)**
    and **Save validation** → the modal closes, the tab bar's indicator turns to the globe « All »,
    and the tour moves on. The step is skippable: skipping it leaves a test pipeline (flask, 0). Every card the tour has you create must be **on screen**
    when it asks you to click it. Every edge is drawn from the **border** of the source card (there is
    no output dot), and the two edges out of the tester carry **`out`**, its first output — if one
    carries another port, the card sends you to the edge's **Outputs** section to tick `out`, and
    only then advances. Before Save, the tour has you **connect Start to implementer**: without
    that edge no node of the pipeline could start (the implementer's only other way in is the
    tester's conditional edge). **Midway (e.g. on *Name it tester*), press `‹ Previous`, then `Next`** → the
    step before comes back and waits for `Next` (its condition already holds), and the tour carries
    on to its end card as usual, with no « this step could not be completed » card. **On *Select
    the node***, `‹ Previous` is greyed out: *Choose Node* needs the Add menu, which closed on the
    pick. The last step, *Keep it, or delete it*, takes Delete or **Skip** (Skip keeps the pipeline)
    before the end card.
15. Open **Settings › General › Tutorials** → all three tours are ticked, *Overview* first;
    hovering a checkmark reads « Completed <date> ».

## Checks

### Surface

- The welcome modal appears **only** on the fresh-browser + Run-less combination; it never comes back
  after being answered, including after a reload.
- At every step: the target is lit, the rest of the window is dimmed, a click outside the target
  changes nothing, and the progress reads `n/N` (e.g. `3/18`) with the way out in the card's corner.
- No step advances on a timer: leaving the screen alone never moves the tour on.
- Hovering the tour Run's row on the status-icon card does **not** make the card jump: the lit
  area keeps its place while the icon turns into the select ring. Hovering the icon itself, and
  only on the tour Run's row, moves the card on.
- The panel a gesture opened is **not dimmed**: its tabs switch and it scrolls, yet typing into
  one of its fields, ticking a box or pressing one of its buttons changes nothing. Once the tour
  has moved past that step, the panel is editable again.
- The tour never walks past an open output viewer: the card after *Open its output* asks for it to
  be closed.
- Every *Overview* card finds its target in the shipped UI: the right pane is open whenever the card
  points into it, and the node inspector's tabs stay clickable in the read-only panel. (On the core
  alone there are no extension tabs; under a Distribution, its `detail-tab-ext-*` tabs must stay
  clickable too.)
- The launched Run is real — it is in the run list, on the training repository, with a live tmux
  session, and it completes.
- `notes.txt` in the training repository carries the line the prompt asked for, and the node's `out`
  artifact holds the agent's summary.
- The recap names **what happened**: the Run's own name, whether the completion was released, and
  where the node got to. A node still running is never described as finished.
- Settings › General › Tutorials shows a checkmark on all three tours, the day each was finished in
  its tooltip.
- *Overview* reaches its end **without any harness** involved: its Run's two nodes are scripts, its
  canvas takes `implement-review`'s layout, and the Run takes reviewer → End.
- The example Trigger is **active** while the tour shows it, yet never creates a Run, and it is gone
  from the Triggers tab once *Overview* ends. The `tutorial-overview` pipeline stays in the Library.
- Every name the tours prepare or hand over carries the **same** `<id>` suffix (`tour-overview-<id>`,
  `tutorial-example-trigger-<id>`, `my-first-run-<id>`, `tutorial-implement-test-<id>`), and a
  reload keeps it.
- **The handover leaves nothing behind**: after *Continue*, no artifact modal is still up, and the
  first step of *First pipeline* takes a real click — an overlay that survived the chain would dim
  into the background and eat every click the next tour asks for.
- Each node the reader creates in *First pipeline* lands **clear of the others** and of the End
  marker, so the edges the tour then asks them to select can actually be clicked.
- **Every lit target is reachable with the mouse, at an ordinary window size** — no step asks for a
  click outside the visible canvas, and no step needs a wider window, a zoom-out or a pan to be
  satisfied. The tour overlay owns the wheel and the drag, so a target off screen is a dead end.
- No card mentions a dot or a handle to drag from: every edge step names the card's **border**.
  On a node with **two outputs**, the card names the one the edge must carry (`out`); an edge that
  carries only the other one does **not** count as the step done — the card re-aims on that edge,
  then on the panel's **Outputs** section, and advances once `out` is ticked.
- A `Create` the daemon refuses **says so in the dialog** (replay the tour keeping the pipeline it
  builds: the name is then taken), and the tour stops on its refusal card rather than waiting on a
  button that does nothing.
- The tutorial Run's canvas shows Start, the agent and End **side by side**, none on top of another.
- **Previous never stops a tour**: a replayed step is shown again (waiting for `Next` when its
  condition holds, live on its target otherwise), and `Previous` is greyed out, with a tooltip
  saying why, when the step's screen is gone. No failure card ever follows a `Previous`. There is
  no `Previous` on a tour's first step, on the intro, stop and end cards, nor from the first step of
  the next leg of the full tour back into the one before.
- **← and → are left to whatever has the focus**: in a New Run field or the pipeline name field,
  ← moves the caret and the tour does not move; → does nothing while the card shows no live `Next`.
  `Next`'s tooltip reads « Next (→) ».
- In *First run*'s pipeline menu, `tutorial-interactive` sits under « Project pipelines · Tutorial »
  and is the default; the pipeline *First pipeline* creates is born a **test pipeline** (flask, 0 in
  the tab bar and on its row of the Pipelines tab) until the Validate step, then reads « All ».

### Internals

- The training repository at `/tmp/pdo-tutorial` is a git repository with one commit on `main`.
  Replaying the tour does **not** add a commit, and does not overwrite an edit made to the tutorial
  pipeline.
- Nothing in the daemon mentions tours: the repository came from the generic repo-creation verb, the
  pipeline from the ordinary pipeline API, the Projet « Tutorial » from the ordinary Projet API.
- `GET /pipelines` gives `tutorial-interactive` `{"kind":"projects","project_ids":[<Tutorial>]}`,
  and `GET /projects` lists « Tutorial » once with `/tmp/pdo-tutorial` among its members. Replaying
  the tour reuses that Projet instead of creating a second one.

## Cleanup (best-effort)

- The Run and the training repository are throwaway; `/tmp` takes care of the repository.
- Clear the tutorial memory with **Settings › General › Tutorials › Reset** (« Forget the
  checkmarks and the welcome prompt of this browser ») to run the journey again in the same browser.

## Notes

- **A brand-new folder may make the harness ask for trust** the first time it opens a session
  there (the demo daemon trusts `/tmp`, so it usually doesn't). Answering it is part of step 7, not a
  failure: its cursor starts on « ❯ No, exit », so pick « Yes » (Down, then Enter). A bare Enter
  ends the session, and the tour then waits on a dead node.
- **Quitting a tour marks nothing.** A run of this journey that stops halfway must not leave a
  checkmark behind — if it does, that is the finding.
- The chain of a full tour lives in the page, not on disk: reloading mid-way ends the chain (a stated
  v1 limit, not a finding).
- Agents are not deterministic. The check is that the node **finished** and wrote its output, never
  the wording of the summary.
- **Do not assert the product's name or logo.** The welcome modal, the tour cards and the training
  prompt name the product from the binary's **Marque** (CONTEXT.md): the core shows « PDO », a
  Distribution its own. The journey checks the cards' gestures and targets, never the brand.
