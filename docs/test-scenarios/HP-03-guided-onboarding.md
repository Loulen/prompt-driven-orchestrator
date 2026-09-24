---
id: HP-03
covers: [tour, tour-step, projecteur, training-repository, welcome-modal, run, interactive-node, completion-guard, artifact]
---

# HP-03 — A newcomer takes the full tour

## Goal

Somebody opens PDO for the first time and is offered the **full tour**. Taking it, they launch a real
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
- **Interactive node** and its **completion guard** (ADR-0068): the node cannot end until a human
  releases it, and the release is observable in the inspector.
- **Artifact modal** on the node's `out` port — the markdown the agent wrote.
- **Settings › Tutorials** (#823): a checkmark per finished tour, in this browser.

## Preconditions

- The app is running locally and reachable in a browser; the status bar shows the daemon
  **connected**.
- A **harness is configured and on PATH** — the tour launches a real Run with a real agent. Without
  one the tour stops on its refusal card, which is correct behaviour but not this journey.
- **A browser with no tutorial memory** (a fresh profile, or the tour keys cleared) and an instance
  **with no Run**. Both halves matter: the welcome modal is the entry point being tested.
- No hard-coded ports, ids or paths in the journey — see `docs/agents/run-scenario.md` for how to
  drive PDO.

## Journey

1. Open the app on a Run-less instance in a fresh browser → the **welcome modal** offers the full
   tour, each tour on its own, and « later ».
2. Choose the **full tour** → the *First run* tour opens on its card, which names what it is about to
   do and shows a line per **preparation** (training repository, training pipeline). `Start` stays
   dead until both have answered.
3. `Start` → the first step lights the **New Run** button and nothing else; a click anywhere else
   does nothing at all.
4. Walk the form, doing exactly what each card asks: name the Run, pick the training repository
   through the **magnifier** (the explorer opens on the folder the tour created), choose the tutorial
   pipeline in the **pipeline menu**, pick the `Default` agent profile, tick **both** PDO skills in
   the skills picker — which stays open across the two ticks, the card's checklist ticking as you go,
   and folds back to « 2 active skills » — paste the prompt → each step advances **on the gesture**,
   with no extra confirmation but the ones the card asks for.
5. **Launch** → the Run appears in the list on the left and the tour points at its row. Launch also
   opens the Run's tab by itself, so the card says so and waits for a `Next` — the step is shown,
   never skipped past.
6. Open the **node** on the canvas → the inspector shows its live terminal.
7. **Talk to the agent**: the card hands over a line to copy; paste it into the terminal and press
   Enter. **Escape inside the terminal belongs to the agent** — it must not end the tour.
8. Click **Mark ready for completion** → the tour advances on the released guard, not on the click.
9. The **wait** step lights the whole inspector as a soft zone — the terminal stays readable and
   usable — shows a two-item checklist (released / finished) and says it has **no time limit**. It
   ends by itself when the node's status becomes terminal.
10. Open the node's **`out`** output → the artifact modal shows the summary the agent wrote, lit as a
    soft zone with the card beside it. Read it, then `Next`.
11. The **intermediate card** says how far along the full tour is, recaps what was actually observed,
    and offers *Finish here* or *Continue · First pipeline*.
12. **Continue** → the artifact modal closes and the *First pipeline* tour starts on a clear screen.
    Carry it to its end card and `Finish`. Every card the tour has you create must be **on screen**
    when it asks you to click it. Every edge is drawn from the **border** of the source card (there is
    no output dot), and the two edges out of the tester carry **`out`**, its first output — if one
    carries another port, the card sends you to the edge's **Outputs** section to tick `out`, and
    only then advances.
13. Open **Settings › Tutorials** → both tours are ticked.

## Checks

### Surface

- The welcome modal appears **only** on the fresh-browser + Run-less combination; it never comes back
  after being answered, including after a reload.
- At every step: the target is lit, the rest of the window is dimmed, a click outside the target
  changes nothing, and the progress reads *n / N* with the way out in the card's corner.
- No step advances on a timer: leaving the screen alone never moves the tour on.
- The launched Run is real — it is in the run list, on the training repository, with a live tmux
  session, and it completes.
- `notes.txt` in the training repository carries the line the prompt asked for, and the node's `out`
  artifact holds the agent's summary.
- The recap names **what happened**: the Run's own name, whether the completion was released, and
  where the node got to. A node still running is never described as finished.
- Settings › Tutorials shows a checkmark on both tours, with the day each was finished.
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

### Internals

- The training repository at `/tmp/pdo-tutorial` is a git repository with one commit on `main`.
  Replaying the tour does **not** add a commit, and does not overwrite an edit made to the tutorial
  pipeline.
- Nothing in the daemon mentions tours: the repository came from the generic repo-creation verb, the
  pipeline from the ordinary pipeline API.

## Cleanup (best-effort)

- The Run and the training repository are throwaway; `/tmp` takes care of the repository.
- Clear the tutorial memory from **Settings › Tutorials › Reset tutorial memory** to run the journey
  again in the same browser.

## Notes

- **A brand-new folder makes the harness ask for trust** the first time it opens a session there.
  Answering it is part of step 7, not a failure.
- **Quitting a tour marks nothing.** A run of this journey that stops halfway must not leave a
  checkmark behind — if it does, that is the finding.
- The chain of a full tour lives in the page, not on disk: reloading mid-way ends the chain (a stated
  v1 limit, not a finding).
- Agents are not deterministic. The check is that the node **finished** and wrote its output, never
  the wording of the summary.
