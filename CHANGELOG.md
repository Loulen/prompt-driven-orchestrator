# Changelog

One entry per version, newest first: what changes on screen, the new seams for extensions and the
breaking changes, each marked **Breaking**. The binary embeds this file: "What's new" shows its
latest entries. The full history lives in the git log and in the
[GitHub Releases](https://github.com/Loulen/prompt-driven-orchestrator/releases), generated from
the commits. The history of PDO 1.x, up to 1.132.0, is archived in
[docs/CHANGELOG-v1.md](docs/CHANGELOG-v1.md).

Work not released yet sits under `## Unreleased`, the first entry; "What's new" does not show it.
`make release-bump` turns it into the version's entry when a story merges into `main`.

The project follows semantic versioning. It has **no backward-compatibility goal**: a break is
announced here, never hidden by keeping dead fields. One constraint is not negotiable: **historical
data stays readable**. An archived Run opens and is costed whatever version wrote its payload.

## Unreleased

### Routing: a branch not taken stays dead downstream

- A run whose only path to End goes through a branch that was not taken stays "awaiting you"
  with the reason "unrouted" until you force a route, instead of finishing green with End
  "reached". The same holds for a bounded loop that runs out of laps while its exit goes through
  a branch not taken: the run stays "exhausted".
- A node where branches converge receives only the branches that fired. An input whose branch was
  not taken is no longer listed in the node's I/O; its preamble says "not taken", and a `script`
  gets an empty `PDO_INPUT_<PORT>` (an input of the same name that did fire keeps its path).
- A node you skip yourself (`pdo node skip`, with or without `--output`) still feeds what comes
  after it, as if it had run. The ▶ of a skipped node is unchanged.
- The run state marks a node skipped because its branch was not taken with `pruned: true`.

### Authoring: fixes from the vanilla acceptance tour

- Importing a Claude Code workflow lists what it could not map in English, in the import modal and
  on `pdo pipeline library import`, and the placeholder it writes into a node's prompt is English
  too ("⟨TODO: prompt built at run time — write this node's role⟩").
- Deleting an agent profile something still names says "1 reference moves to the next tier", and
  "2 references move" from two on.
- The Undo tooltip names a note or a loop added or deleted ("Undo add note", "Undo delete loop"),
  as it already did for nodes and edges; "edit note" is left for a note whose text changed. The
  Redo tooltip names the change it replays the same way ("Redo delete note").
- A "changed outside" or library-assistant line above the canvas goes away once you save the draft
  it changed as a version: that version holds the change, and "See v2" no longer points at a
  version older than the one you just saved. A line that arrives while the save is under way stays,
  and so do the lines in the other participants' tabs.
- A node whose agent profile was deleted names the profile as you knew it ("Agent profile Reviewer
  fast no longer exists"), in the warning above the canvas, on the node card's tooltip and in the
  inspector, wherever this browser listed the profile before; elsewhere it still shows the id.
- The import window's second tab reads "Claude Code workflow".
- A double-click on a pipeline in the library opens one tab, not two tabs on the same pipeline that
  each kept their own Undo history. A second open that answers late leaves the open tab and its
  edits alone.
- Add › Note places the new note clear of the notes already on the canvas, as it already did for
  nodes, instead of on top of the last one. A new node keeps clear of the notes too.
- `pdo skill export --skill` takes a skill's name as well as its id, as every other `pdo skill`
  command does.
- `pdo settings agent-profile delete --help` says what the daemon does: it deletes a profile even
  while something names it, and each of those falls back to the next tier.
### Upgrading from 1.x

This is the first 2.x release published in this repository: it replaces PDO 1.x in place.

- Your data is kept. 2.x reads the `~/.pdo` folder of 1.x as it is: pipelines, Runs, Projects,
  settings and the database open without a migration step.
- The service is kept. The same unit starts the daemon at boot, now on the 2.x binary.
- To upgrade, run `brew upgrade pdo` (or `brew update && brew upgrade Loulen/tap/pdo`), or re-run
  the install script of the latest release. The Update button of a 1.x install does the same.
- Going back to 1.x afterwards is not guaranteed: 2.x adds tables and columns 1.x does not know.
- From this version on, PDO is licensed under the Apache License 2.0, with a `NOTICE` file. Versions
  up to 1.132.0 stay under the MIT License; their changelog is in `docs/CHANGELOG-v1.md`.

### Updates come from the public repository

- Settings › Version & update checks the latest release of `Loulen/prompt-driven-orchestrator`,
  and the tooltip of "GitHub Releases" next to the check date shows the address it asked. The
  Homebrew command is `brew update && brew upgrade Loulen/tap/pdo` (the `pdo` formula, no longer
  `pdo-v2`), and the script command re-runs that repository's installer.

### Review and observation: fixes from the vanilla acceptance tour

- Review: a node that runs in the Run's worktree (not isolated) and commits its own work now has a
  before → after pair that holds those commits. Before, its delivery started where the node stopped,
  so the pair read "No changes". `NodeStarted` of such a node carries `run_head_sha`, the Run
  worktree's HEAD when it started; a node started before this version keeps the old pair.
- Stats › Cost and Run info: a Run that started no Pipeline Manager and no merge resolver no longer
  counts an "Infrastructure" execution of unknown cost. Before, every Run read "without computable
  cost", the Cost headline median stayed "—", and a script-only Run counted a `claude` execution.
- `pdo stats` reads 30 days when `--period` is not given, the period Stats opens on (it read 7).
- `pdo review diff --from/--to` with a revision git does not know answers `unknown ref: <ref>` and
  lists the Run ref spellings, instead of "run branch not found"; `--help` lists them too.
- `pdo complete` refused for a frontmatter mismatch says the agent has one retry left (it said the
  retry was spent). A Run parked by a node's `pdo fail` names the node ("node make failed (awaiting
  your confirmation)") instead of saying "agent pdo fail", which read wrong for a script node.
- Stats › Performance: the wait coverage reads "1 execution of 4" and capitalises its second
  sentence.
### The interface takes the Marque of its binary: PDO, or Acogni Driver

- The vanilla binary now calls itself **PDO** everywhere it names the product: the browser tab
  (title and favicon, written into the page by the daemon so no other name flashes on load), the
  mark at the left of the top bar, the welcome modal ("Welcome to PDO", the PDO lockup), the tours,
  the import of a pipeline ("PDO pipeline"), the notifications, and Settings › Version & update,
  whose Installed row reads `PDO 2.76.0` after the mark. A distribution that declares Acogni
  Driver shows "Acogni Driver" and its logo instead, and `Acogni Driver 1.4.0 (core PDO 2.76.0)`.
- New PDO logo: three steps of a Run (done, running in ruby, to come) joined by two edges. Acogni
  Driver's mark is the A of Acogni, its hollow in ruby. Both keep their ruby under the Classic
  Theme, in dark and light.
- The gear of Acogni is the Settings icon now, in the top bar and in the Settings header; it no
  longer stands for the product, so the top bar never shows two gears. It signs "by Acogni" under
  the PDO lockup of the welcome modal and after the title of Settings › Version & update.
- The Acogni wordmark of the Acogni Driver lockup is no longer clipped under its "g".
- New seam for distributions: `Config::with_brand(pdo::Brand::AcogniDriver)`, next to
  `with_release_channel`; without it a binary is PDO. Neither an environment variable, a setting
  nor an extension changes it. The daemon serves the logos of its Marque under `/shared/brand/`
  (`mark.svg`, `lockup.svg`, `favicon.svg`) for the pages an extension serves outside the
  interface; `/shared/favicon.svg` now answers the Marque's favicon.
### Runs: a pause keeps the work in flight, a reopen relaunches stopped nodes

- A node that finishes while its Run is paused now completes. Before, its completion was refused
  ("resume the run first"): a `script` node, which completes once as its bash exits, lost its work,
  and after Resume it read "session died" and had to run again. What the finished node would launch
  still waits for Resume.
- Reopening a Run (the Play button, `pdo run reopen`) runs again the nodes that were stopped, as it
  already did for interrupted ones. A cancelled Run (`pdo run cancel`) used to park "run_stalled …
  blocked behind" its stopped nodes as soon as it was reopened. A stopped lap of a collection
  fan-out stays stopped, with the laps already done, until its node is retried.
- A `script` lap of a collection fan-out gets its item in `$PDO_ITEM` and the number of laps in
  `$PDO_ITEM_TOTAL`, as an agent lap reads them in its preamble.

## 2.75.0

### Tours: Previous replays the step before, and the arrow keys drive the card

- From step 2 on, a tour card shows "‹ Previous" on the left of its footer, before Skip. It
  replays the step before as an ordinary step: shown again and waiting for Next when what it asked
  for is already done, or live on its target when the gesture has to be made again. When that
  step's screen is gone (a dialog, a menu or a viewer closed since), Previous is greyed out and its
  tooltip says "This step's screen is gone: it can't be replayed"; the tour reopens and undoes
  nothing. There is no Previous on a tour's first step, on its intro, stop and end cards, nor from
  one leg of the full tour back into the one before.
- ← presses Previous and → presses Next, each only when the card shows it live, and only while no
  field, terminal, canvas, list, menu or viewer has the focus or is open. Next's tooltip reads
  "Next (→)".
- *First pipeline*: a condition step replayed while another edge is selected lights its own edge and
  says when its condition is already set; "Create it" says "Already created" however the
  pipeline was created, and Save and Validate replayed say they are done; the last step notices the
  pipeline is deleted even when its tab closed with it. *First run*: Launch replayed after the
  launch says what the click did.
### Cost cap of a loop changed while the run goes on

- "raise cap", on a loop region that reached its cost cap or in Run info's "Budget reached"
  banner, opens Edit › Edit for this run with the region inspector on `max_cost_usd`, focused. The
  inspector takes an amber outline and an "Editing for this run" pill; its header reads the cap the
  run applies. A cap given as a `$variable` shows the amount the run resolved (`$ 5`); the YAML
  keeps the variable until you type another cap.
- A run open in "Edit for this run" follows a cap changed outside it (the CLI, an extension, another
  browser): an untouched cap takes the new value, and Save only sends a cap you changed, so it
  never puts the old one back. A cap you already typed keeps your draft.
- While editing a run, the cap field says "Applies live. Saving resumes the Run at the next lap."
  A cap under what the loop already spent is allowed, with an amber note: the loop stops at its
  next re-entry and the lap in progress always finishes. ✕ removes the cap. On a run waiting
  "budget reached", a note says how to go on, with "end region" at the bottom.
- Save applies each changed cap to the run: raised over the spend, the next lap starts at once.
- New run command `set_region_max_cost` (`max_cost_usd`: dollars, or `null` to remove the cap),
  sister of `set_region_max_iter`: the change is kept by the run (`region_max_cost_overrides` on a
  run read, kept across a reopen) and beats the pipeline's `max_cost_usd`, a `$variable`
  included. Refused with a 400, nothing recorded, on a collection, an unknown region or a value
  at or below 0. Extensions see it as the Run Operation `set_region_max_cost`, granted like
  `set_region_max_iter`; the Pipeline Manager's guide documents both.
- `pdo run region <run> <region> --max-cost <usd|none>` sets or removes the cost cap from a shell.
- The "budget reached" reason now names `set_region_max_cost` as the way to raise the cap.
- The "budget reached" and "exhausted · unrouted" blocks of a loop region stay clickable when an
  exit edge's condition label sits on them. An edge whose `when` is a bare string shows that
  string, no longer "0 and 1 and 2 …", and renaming a port no longer rewrites it.

### Waiting alerts: a notification and a sound when a node starts waiting for you

- Settings has a new Notifications category (after Appearance) with a "Waiting alerts" section,
  kept by this browser and saved at once, outside the Save of the instance settings. "Browser
  notification" and "Sound" are two switches, off by default, each with a Test button. Turning the
  notification on asks the browser for its permission; the line under the switch says where it
  stands, and on http through a local network address it says notifications need https or
  localhost, while the sound still works.
- "Alert me for" chooses which Runs alert this browser: "Runs I launched" (the default, with the
  Runs they launched), "Projects" (the Runs launched in the Projects you check) or "All I can see".
  It only narrows what your rights already show.
- When a node of such a Run starts waiting for you (a question, a completion to approve, a Run
  paused on an incident) and no tab of the interface is in view, one tab shows a notification
  (the node's name, then "is waiting for you" or "is done, waiting for your go"; "Run paused on an
  incident") and plays a chime of its reason, at most one chime every 10 s. Clicking the
  notification brings the tab back on the Run with the node selected. A wait already there when
  the page loads, or caught up after a reconnection, alerts nothing; neither does a parent node
  waiting on its child runs.
- `GET /runs` gives each child Run its `root_run_id`, the Run at the top of its chain of parents.
- While an alert switch is on, a hidden tab of the interface reopens its event stream after a drop (a daemon restart,
- While an alert switch is on, a hidden interface tab reopens its event stream after a drop (a daemon restart,
  a network change) instead of waiting to be shown; `openProbedSocket` takes a `whileHidden` option.
### Tree cost: what an orchestration really cost

- A Run that launched child runs shows its tree cost in Run info: the "Est. cost" cell reads the
  Run's own cost plus every descendant's (archived ones included), followed by "⑂ N", the number
  of descendants. Its tooltip breaks it into "Parent run" and "Child runs (N)", and into each
  depth when the tree is deeper than one level. A descendant whose cost cannot be computed turns
  the total into "—" and says how many runs are missing, the known parts kept; a lower-bound
  descendant makes the tree a lower bound (†). A Run without child runs shows its cost as before.
- The Orchestration tab shows each child's tree cost, with a "⑂ N" chip on a child that has
  child runs of its own; Σ sums the children's tree costs, and a "Tree cost" line under it gives
  the Run's total, parent run plus child runs.
- `GET /runs/{id}` carries an additive `tree_cost` (total, parent, children, descendants,
  by depth) for a Run that has descendants, and each entry of `GET /runs/{id}/children` its own.
  For extensions, `RunCost` gains `tree` (`RunTreeCost`: total, parent, children, descendants,
  by depth), read by `Host::run_cost`.
### Stats › Cost: roll up child runs under their root's pipeline

- The Cost band has a new "roll up child runs" chip, after a divider, off each time Stats opens;
  its (i) says the rule. Ticked, every child run counts under the pipeline, the project and the
  period of its root run: a pipeline's cost then includes everything it orchestrated, and a child
  pipeline keeps only the runs nobody orchestrated. A child started after the period comes in
  with its root, and "completed runs only" judges the root. The cohort line turns amber and says
  the totals per pipeline and project differ from the default view.
- A root pipeline that orchestrated child runs shows "⑂ N" in the ranking. Opened, it splits into
  "Parent run" (this run's own nodes) and "Child runs" (N runs · K levels), which unfold in place
  by depth, then child pipeline, then nodes; each level also opens in the breadcrumb.
- "By model" does not take the setting: the chip and its divider leave the band, and come back as
  they were on "By pipeline" and "By project".
- `GET /stats/cost` takes `roll_up_children=true`; each root pipeline row then carries an additive
  `roll_up` (`parent`, `children`, `levels`, `depths` with their pipelines). Without the parameter
  the response is unchanged.
### Any node that creates a child run waits for it

- A node that creates a child run (`pdo run create`) waits for it, with or without the
  Orchestrator toggle, agent or script: `pdo complete`, the end-of-turn auto-completion and the end
  of a script are refused (`children_pending`) while a child is still live, and the node completes
  by itself once every child has ended without failing. A failed child parks it awaiting you.
- The refusal tells the agent to wait with `pdo run wait --all`; it no longer says
  "orchestrator", and `pdo complete` never claims the held node failed. A script whose
  `pdo complete` is refused this way is no longer failed by its own tail.
- On the canvas, the card of any node with child runs shows "N runs" and the counters; its panel
  adds the Orchestration tab next to I/O and Transcript as soon as the first child appears, live,
  leaving I/O open. With the toggle, the tab shows from the start, on "No child run yet".
- The "Waiting on child runs" banner gets an "Open Orchestration" button; once a child failed, it
  turns into "N child run(s) failed" with "Open", toward that child (the Orchestration tab no
  longer repeats it).
- The Orchestrator toggle's tooltip says what it still does: it adds the `pdo-orchestrate` skill
  and shows the Orchestration tab from the start. The Stats keep reading the Orchestrator node kind
  on the toggle.
- A node held by its child runs is no longer marked interrupted when the daemon restarts without
  its session: it keeps waiting on its children and completes once they end.
### Finishing touches: Start card, Settings guard, round selection, colour-blind palette

- The Start card of a Run's canvas no longer shows the input images; their thumbnails and the
  lightbox stay in the Start inspector.
- Update and What's new in Settings › Version & update leave Settings through its draft guard:
  with a modified form, "Close settings?" asks first (Keep editing leaves Settings open). Once
  Settings is closed, the update confirmation or the changelog shows alone, and closing it goes
  back to the app, not to Settings.
- The select control of the Runs, Triggers and Pipelines lists is round: hovering a row shows a
  ring in the colour of its status, a ticked row a full accent round with its check. The
  "select all" of a group header is round too.
- "Colour-blind friendly status colours" in Settings › Appearance › Theme, off by default, part
  of your Apparence: saved to your account, found on every device you sign in from and applied
  before the first paint. It swaps the awaiting, done, failed and blocked colours for yellow,
  teal, rose and beige in Acogni and Classic, light and dark; status shapes do not change.
- Seams: the Apparence document takes an optional `colour_blind_status` (`PUT
  /actors/me/appearance`, `pdo settings appearance`); `/shared/theme.js` stamps
  `data-status-palette="colour-blind"` on `<html>`, and `/shared/tokens.css` swaps the
  `--color-st-*` tokens under it, so an extension page follows without a change.
### Loop cost and cost cap of a bounded loop

- The corner of every loop region shows its loop cost after the counter, in the run cost's format
  (`3/5 · ~$1.20`, `2/4 items · ~$0.84`, `~` an estimate, `†` a lower bound, `—` unknown), counted
  since the loop was entered; its tooltip gives each member's share. A one-member loop shows it on
  its card's badge.
- A bounded loop takes a cost cap, `max_cost_usd` (a dollar amount or a `$variable` resolved at
  launch), set in the region inspector: `$` prefix, "no cap" when empty, ✕ to remove it. A value
  at or below 0, a value that is not a number, and a cap on a collection are refused with a
  reason; a `$variable` that does not resolve to a positive amount refuses the launch.
- The corner reads `~$1.20 / $5` on a run (`max 5 · ≤ $5` on a pipeline), and its cost turns
  amber from 80 % of the cap. Once the loop's cost reaches the cap, the lap in progress finishes
  and the next one does not start: the run waits "budget reached" (code `region_budget_exhausted`,
  the region in `awaiting_region_id`), never failed, and the loop's exhaustion exit is not
  followed. `max_iter` wins when both bounds fall on the same lap. The region turns amber with a
  wallet and a "budget reached" block under its cards, with "raise cap" and "end region". Run info
  explains the stop (spend, cap, lap); the region it names, and its "raise cap", open the region
  inspector in place of the panel. A member running on a harness with no cost source is flagged in
  the inspector ("Cap may be overshot").
- An open live run is read again every 30 s, so its node costs and loop costs climb while an agent
  works, not only when a node starts or ends.
- The region inspector shows a "Loop cost" section on a run: the spend against the cap, its share,
  and each member's cost.
- "exhausted · unrouted" no longer shows during a loop's last lap nor after it left by its exit:
  only while the run waits on that region.
- New fields for extensions and the API: `max_cost_usd` on a loop region, `loop_costs` (with
  `cap_usd`) and `awaiting_region_id` on a run, `can_cost` on each harness of `GET /settings`.
### Tours playable by several people on one instance, and the Status icon in the Overview tour

- Each browser draws a tour identity (four characters, `[0-9a-z]`) once and keeps it. The Runs,
  pipelines and Triggers the tours prepare or have you paste carry it as a suffix:
  `tour-overview-k3x9`, `tutorial-example-trigger-k3x9`, `my-first-run-k3x9`,
  `tutorial-implement-test-k3x9`. A tour only points at, advances on and tidies up what carries its
  identity: someone else's Launch or deletion no longer moves your tour on, and Overview never
  reuses another person's Run. The example Trigger left without a suffix is removed; the Overview Run
  left without one stays.
- "Find your Run" names your row ("Your Run, `my-first-run-k3x9`, is lit in the list on the
  left…") instead of calling it the top row, and the First run recap names the Run this browser
  launched. The step cards set the names a tour prepared in bold monospace.
- The Overview step about status is now "The status icon": it explains the shapes (a check, a
  turning circle, a question mark, a cross) and draws the four Status icons instead of coloured dots.
  It moves on when you hover the status icon of your Run, as its footer says, or with Skip.

## 2.74.0

### Approve ▾ beyond the node panel: enlarged terminal, command field, tour

- The enlarged terminal carries the same Approve ▾ as the panel, in the normal size, in place of
  its two compact icons. It shows whenever the node would complete (running too, not only
  awaiting you), waits in a lighter ruby while an output is missing, and its "Auto-approve when
  ready" entry stays checked when you collapse back to the panel. A refusal opens under the bar.
- The terminal bar says who drives the session with a ruby chip instead of capital letters: the
  pilot reads "You pilot" and how many browsers watch ("2 watching"); a spectator reads
  "Watching" and takes control from "Take control" in the chip, which replaces the lone hand icon.
- The command field offers "Approve" (formerly "Mark complete") and "Auto-approve when ready" on
  the selected node, on the same conditions as the button: Approve greyed with the missing
  outputs, the entry checked once armed, a refusal of your rights greyed with its sentence.
- The First run tour points at Approve ▾ and its "Auto-approve when ready" entry. The completion
  verdicts speak the same words ("Approving…", "Arming auto-approve…", "Auto-approve refused").

### One Approve ▾ in the node panel, waiting for the missing outputs

- The node panel has one completion gesture, Approve ▾, at the right of the I/O / Transcript tabs,
  away from Stop. Approve completes the node now; its menu holds "Auto-approve when ready" (the
  former "Mark ready for completion"), offered on an interactive iteration whose session is live.
  The header keeps Review, Retry, Start, Restart and Stop: the "Mark ready for completion" and
  "Mark complete" icons and the "Mark ready" button are gone.
- Approve says what is missing before the click: while a declared output is not written, it is
  inert, in a lighter ruby, and its tooltip lists the missing outputs. It turns clickable within
  seconds once the agent writes them. A refusal of your rights still greys it, with its sentence.
  Each output card the guard still waits for reads "missing" next to its type, at rest (it
  replaces the "not produced yet" shown on hover).
- When the user has not released an interactive node, the refused `pdo complete` tells the agent
  to ask for "Auto-approve when ready" in the Approve menu, or Approve; the seeded
  `pdo-interactive` skill (version 3) says the same.
- Once "Auto-approve when ready" is armed, its entry is checked and a blue dot sits on the chevron.
  No banner says "Completion released" or "Approved" any more; a refusal is still shown.
- "Done, awaiting approval" loses its buttons: its "Show Approve" brings Approve ▾ to the screen
  with an amber ring. The tabs scroll in a narrow panel; Approve ▾ stays pinned and keeps its
  label.
- New seams: `missing` on each output of a node's I/O read (`GET /runs/:id/nodes/:node/io`, the
  `PortIO` type), by the completion guard's own rule (an inherited file counts as present); the
  kit's `SplitButton`.

## 2.73.0

### A refused gesture is greyed, with a sentence that says why

- A gesture you may not make stays on screen, greyed, instead of disappearing: Stop, Retry, Mark
  complete and the other gestures of a Run and its nodes, a row of the Runs list and its menu, the
  bulk bar, the canvas, the command field, the extensions' actions, New Run, Projects, Settings,
  Agent profiles, staging profiles, the Skills bank, Triggers, Review comments, Stats absorptions
  and the Pipelines list. Its tooltip says what you cannot do and whom to ask; in a menu, one line
  at the menu's foot says it, with a lock. Nothing is greyed without an extension that restricts.
- A refusal from the daemon reads as that sentence, never as the extension's technical reason. Without a
  sentence from the extension, it reads "You don't have the rights to do this.". `pdo` prints the
  sentence followed by the technical cause in parentheses.
- New seams (extension API 0.23): `Authorization::deny(reason).saying(sentence)`,
  `Denied::new(reason).saying(sentence)`, `Denied::sentence()`, `Denied::for_an_agent()`,
  `Denied::from_body()` and `GENERIC_REFUSAL`; `POST /authorize` and the `403` `operation_refused`
  body carry `sentence` beside `reason`. In `@pdo/extension-ui`, `disabledReason` on `Button`,
  `IconButton`, a `Menu` entry (the menu shows it at its foot) and a `Segmented` item; a `Segmented`
  item's `title` and `description` show in the kit's `Tooltip`. An action guarded by an Operation
  is greyed when refused, no longer left out.
- **Breaking for extensions** (`pdo-extension-api` 0.23, Rust API): `Authorization::Deny` and
  `Denied` gain the field `sentence`. Match
  `Authorization::Deny { reason, .. }`, build `Denied::new(reason)` and `Authorization::deny(reason)`.

### A session's variables out of the process list

- The variables of a session (a Node's, those an extension adds through `node_env` or
  `session_env`, the core's) no longer sit in the argv of any process, where any local user read
  them with `ps` while the session lived. The session's script reaches `bash` through a `0600`
  file in the user's private folder, deleted before the harness starts; `tmux` only sees its path.
  In a sandbox, the `docker exec` forwards the variables by name (`-e NAME`), their values
  exported on the host just before. Every launch is covered: a Node, its Retry and its resume, a
  hosted session, the Pipeline Manager, the library assistant.

### A Presence falls within seconds, behind a proxy too

- A person who leaves a Run, closes a tab or loses the network no longer stays on the Run's row:
  behind a proxy or a tunnel, a connection the browser lost could stay open on the daemon's side
  and keep the person on the previous Run until a reload. Each tab now names itself on `/ws`
  (`/ws?tab=<id>`): its new connection replaces the old one, whose Presence falls at once. The
  daemon pings every connection of `/ws` and drops one that answered nothing for 10 s.
- Extension API: the life of a `/ws` connection (`tab`, the close code `4000` of a replaced
  connection, Ping and the 10 s delay), see the reference's Presences.

### Who made each gesture on a Run

- The events of a gesture on a Run name the Actor who made it, as `payload.actor` (the shape
  `node_resumed` already used): a command of the operator or the Pipeline Manager (mark done, force
  route, skip…), a retry, a stop, a skip, a completion, a fail, a release, starting or stopping the
  Pipeline Manager, and the events of that Run these cause while they are handled (the stop and
  the respawn of a retry, a downstream node a mark done starts). What the scheduler does on its own
  names nobody. Events written before keep reading as they are.
- Two new events on a node's terminal: `node_taken_over` when an Actor takes the hand (attaches
  first, takes control from the pilot, or an extension answers on its behalf; nothing more when the
  same Actor attaches again), and
  `node_typed` for a turn of speech, the first input after a take-over or a wait, never one per
  keystroke. Answering a wait stays the one `node_resumed` line.
- `pdo run events <run>` adds a last column, who made the event: its display name (`POST
  /actors/display`), else its id; empty when no Actor did. `--json` gives `payload.actor`.

### A page mounted from a node is judged in the Projet of its Run

- New seam (extension API, a minor): when a node session mounts a page (`pdo page mount`,
  `POST /pages`), its `page_mount` Operation is in the Projet of the session's Run; the unmount of a
  page a Run mounted is in the Projet of that Run, whoever asks. An extension that grants writing in
  that Projet now lets an agent publish a page without any instance-wide grant. A mount outside any
  Run still names no Projet. `POST /authorize` places a `page_mount` question the same way.

### A link toward a Node, and an answer typed on someone's behalf

- A deep link toward a Node opens the Run with that Node selected and its detail shown, its
  terminal included, Pipeline info giving way: `#/runs/<run>/nodes/<node>`, and
  `#/runs/<run>/nodes/<node>/iter/<n>` for one of its iterations. A Node the Run does not hold
  opens the Run alone. A page's reading guide (`pdo page mount --guide`) accepts the iteration form.
- New seam (extension API 0.23, a minor): `Host::write_to_node(actor, NodeWrite)` types a text into
  the session of a Node that waits for a person and presses Enter, on behalf of the Actor, who must
  be allowed to take control of that session (`session` × `take_control`). The Node resumes, its
  resumption names the Actor, and the Actor is announced as taking control (`session_taken_over`).
  A Node that no longer waits refuses with the stable reason `not_waiting` ("the node no longer
  waits"), and nothing is written. `node_link` and `run_link` write the deep links;
  `@pdo/extension-ui` writes them too (`nodeLink`, `runLink`).

## 2.72.0

### Settings, Stats and extension pages open under the top bar

- Settings, Stats and the pages of the top bar's extension entries now open under the top bar
  instead of covering it: the command field, the icons and the account stay on screen and usable.
  The icon of the open surface is selected (`aria-pressed`, a 2 px stroke under it); a second click
  on it closes the surface, as do its ✕ and Escape, and another icon switches to its surface. A deep
  link toward a page opens it the same way. Settings with unsaved changes still asks before closing,
  also when another icon replaces it: "Keep editing" keeps Settings on screen.
- Extension pages: the core's command field stays above every page, so a page no longer needs a
  search field of its own in its header.

### New Run, opened prefilled by an extension

- An extension's interface can open the New Run window prefilled: Projet, repositories, name and
  prompt show in the form and stay editable, and only the calling extension's `run/new` section
  receives its own data, to start from (items already checked, say). The Pipeline stays your
  choice. A Projet the form does not offer you is ignored, and the default one applies.
- New seam (extension API 0.22, a minor): `openNewRun(prefill?)` in `@pdo/extension-ui` and
  `CommandHost.openNewRun(prefill?)`, with the type `NewRunPrefill`; `RunNewContext.prefill` hands
  the data to the caller's `run/new` sections, which provide it to the creation as before. Without
  a prefill, the form opens as the core's New Run button opens it.

### Command line

- **`pdo project`** makes every gesture of Settings › Projects: `list` (and `--deleted`), `show`,
  `create` with its member repositories, `update` (name, harness or agentic profile, auto-fail,
  primary repository), `member add|remove`, `skill list|add|remove|set`, `provisioning` (show, or
  replace with `--file`) and `delete`.
- **`pdo trigger`** makes every gesture of the Triggers surface: `list`, `show`, `create`,
  `update`, `fire` (Run now), `pause` and `resume` of one Trigger or of all (`--all`), `fires`,
  `test-guard` (a guard's dry run), `health` and `delete`.
- **`pdo session`** reads the status bar's gauges (`status`) and makes the gestures of a hosted
  session's detail: `show`, `events`, `terminate`, `relaunch`, `configure`. **`pdo audit`**
  reads the journal of the mutations made outside a Run, **`pdo whoami`** the Actor the daemon
  names you.
- A Projet and a Trigger only gain skills of the bank: the daemon refuses another one (400, so
  `pdo project skill add|set` and `pdo trigger create|update --skill` exit 1), as the selectors
  only offer the bank's skills. A skill already selected and since deleted from the bank stays.
- These commands print text by default and the daemon's answer with `--json`. They exit 0 on
  success, 1 when the daemon is unreachable or refuses the call (with its reason), 2 for an
  unreadable command line, 4 when the daemon knows no such Run, node, pipeline, Projet,
  Trigger, session, review comment, profile, skill, folder or update attempt.
- **`pdo run`** makes the gestures of the Runs list and the run view: `show`, `cancel`, `pause`,
  `resume`, `retry` (Retry all), `reopen`, `archive`, `forget`, `rename`, `repos`, `inject`,
  `force-route`, `region`, `manager start|stop`, and reads a Run's `events`, `costs`, `artifact`,
  `file`, `refs`, `source-drift`, `search` and the sweep's `stale-health`. **`pdo run pipeline`**
  shows, exports, saves a Run's own pipeline and copies it over the shared one
  (`overwrite-default`, `--preview`). `run create`, `wait`, `list` and `status` are unchanged.
- **Cancel a Run.** `pdo run cancel` gives up on a live Run: nothing more is launched, every node
  holding a session is stopped, and the Run ends `halted` with the reason `cancelled`, ready to
  be retried or reopened. The daemon's new `cancel_run` command (an Operation `run` ×
  `cancel_run`) does it; the UI does not offer it yet.
- **`pdo node`** makes the gestures of a node: `retry` (and `--preview`), `skip` (with its
  outputs), `restart`, `start`, `stop`, `kill`, `recover`, `done`, `release`, `extend`, `attach`
  files, and reads its `prompt`, `io`, `diff` and `transcript`. Without `--iter`, a gesture
  targets the node's latest iteration, the one the node panel shows.
- **A node the Run does not know is refused.** Every gesture on a node (retry, skip, restart,
  start, kill, recover, done, release, extend, and the Pipeline Manager's node commands) answers
  `404 node_not_found` when neither the Run's pipeline nor its state names the node, before
  anything is written: a mistyped id no longer reopens a finished Run or leaves a ghost node in
  it. **Breaking**: retry, restart and `extend_cycle` answered `400` for an unknown node; `pdo
  node` exits 4.
- **`pdo pipeline`** makes the gestures of the pipelines and the library: `list`, `show`,
  `export`, `import` (a round trip, skills sidecar included), `versions`, `version`, `restore`,
  and the editor's other gestures. `restore` is the daemon's new
  `POST /pipelines/{id}/versions/{n}/restore`: the version saved as the next one, under the
  pipeline's current name, as Restore then Save in the editor. A version the pipeline does not
  have answers `404 pipeline has no version N`.
- **`pdo settings`** reads and changes the instance settings as Settings does: `get [<key>…]`
  (key, effective value, source on every line; the instance's skills and agentic profile as
  stored, source `stored`) and
  `set <key>=<value>…`, validated by the daemon as for the UI (a refused change writes nothing,
  exit 1; an accepted one prints the settings changed); the agentic profiles (`agent-profile`), the staging
  profiles (`sandbox-profile`), the instance's provisioning rules and their preview
  (`provisioning`), your Apparence (`appearance`), the telemetry export (`telemetry`, `--test`),
  a harness's catalogue (`refresh-harness`) and the price table (`sync-prices`).
- **`pdo stats`** reads the tabs of Stats with the figures they show (`runs`, `sessions`,
  `triggers`, `cost`, `performance`), over the surface's periods (`--period 7d|30d|90d|all`, or
  `--from`/`--to`) and filters (`--completed-only`, `--every-run`, `--uncombined`), and manages
  the absorptions (`absorptions`, `combine`, `uncombine`). Performance prints the Pipeline rows
  and the Infrastructure row, and reads the duration the tab's toggle picks (`--duration
  total|active|waiting`, `active` by default as the tab opens). A Performance period with no
  successful execution says so, as the tab does.
- **`pdo skill`** manages the Banque de skills as Settings › Skills: `list`, `show`, `add` from a
  `SKILL.md`, `update` (rename, move, new content), `remove`, `referents`, the reference files
  (`file add|show|write|remove`), the folders (`folder list|add|update|remove`, and `rescan` /
  `sync` with their Source), `scan` and `import` from a Source, `cancel-scan`, `sources`,
  `export` to a zip and `upload` one. A skill an extension or the core provides stays locked:
  changing or removing it is refused, exit 1. A skill is named by its id or by its name.
- **`pdo update`** reads the version section (`status`, `changelog`), checks now (`check`),
  updates (`apply`) and reads an attempt's journal (`log`).
- **`pdo review`** gains the Review page's gestures: `diff` (the patch, `--stat`, `--json`, all
  three read from the diff the page shows, work not committed yet included; this patch is for
  reading, `--git` prints git's own patch of the committed work for `git apply`),
  `comment <path> <line> --text …` (written and sent to the Pipeline Manager, as the page sends
  it; a line the page does not let one comment, past the file or in a file outside the diff, is
  refused, exit 1, nothing sent), `resolve` and `reopen`. `list` and `reply` are unchanged; the
  new gestures exit 4 for a Run or a comment the daemon does not know.
- The instance's skills selection only gains skills of the bank: `PUT /settings` refuses another
  one (400, `not a skill of the bank: <id>`), as for a Projet or a Trigger. A skill already
  selected and since deleted from the bank stays; emptying the selection is never refused.
- Every route of the daemon has its command or an exclusion written with its reason: a
  structural test fails on a route added without either. No route is a provisional exclusion any
  more; the shared draft of the pipeline editor, a WebSocket, is excluded for good.

## 2.71.0

### Settings › Appearance

- Settings has an **Appearance** category, right after General, in three sections. **Theme** holds
  the Theme and Dark mode, which now follow your account, and the UI scale, which stays with this
  device. **Appearance** holds one block per surface, its switches on the left and a preview
  beside them. Its first block, Run canvas, starts with Compact nodes; Reset to defaults in the
  section's title bar shows how many choices differ, and is greyed when none does. **Interface**
  keeps the behaviour settings (Single-tab mode, Child runs, Wiring grid, Wiring grid size). General
  no longer shows any of them.
- Each row shows its scope: "Your account" (new) or "This device". Every choice is saved at once,
  never by the instance form's Save.
- Theme, Dark mode and Compact nodes are kept by the daemon for each Actor: you find them on every
  device you sign in from. The first load of an Actor with nothing saved yet adopts the values this
  browser held. Other tabs follow a change at once; other devices take it at their next load or
  reconnection. Without an identity extension, the local operator has the one Appearance of the
  instance.
- The command field has a "Customize appearance" entry that opens Settings › Appearance; its
  Compact nodes switch writes the Actor's Appearance.
- New seams: `GET` and `PUT /actors/me/appearance`, the caller's Appearance (a versioned document,
  at most 32 KiB; a malformed one is refused with a `400`, a larger one with a `413`), and the
  exported types `AppearanceDocument` and `ActorAppearance`. The browser keeps its copy under the
  same storage keys: `/shared/theme.js`, and the pages extensions serve outside the interface, are
  unchanged.
- The **Runs list** block comes first in Appearance, with "Always shown: Status, name". Its switches
  show or hide, on every row of the Runs list, the Trigger, the child runs, the node progress (first
  line), the pipeline, the parent link, the duration and the cost (second line), and each mark or
  people contribution of an extension, named "Extension · label". The defaults are lighter: child
  runs, node progress, the parent link and the cost are hidden; extension contributions are shown.
- When every element of the second line is hidden, Run rows take one line. The height comes from
  your Appearance alone, so every row of the list keeps the same height.
- The block's preview is drawn with the Runs list's own rows: your most recent Run, plus your
  most recent triggered Run when no other row has a Trigger, plus a parent with its child, or a
  sample Run without extension marks when you see no Run. Hovering or focusing a switch rings its
  element in the preview; a hidden element comes back faded, with a dashed outline. This works on
  rows that take one line too: the second line comes back while you point to one of its elements.
  A switch that differs from its default says "changed", with the default in its tooltip, and
  "Modified only" finds it.
- New seams in the interface: the core's table of default visibilities for each surface, and the
  filtering of the contributions an Appearance hides at the `run/item` and `run/item/people`
  locations. The extension API is unchanged.
- The **Run canvas** block, "Always shown: Type icon, name, status, child runs of an orchestrator",
  starts with Compact nodes, then shows or hides on every card of a Run canvas the model, the
  iteration, the elapsed time, the cost, the isolation flag, the loop badge and the agent profile
  marker. By default a card shows its elapsed time and its cost on one row, so cards are shorter
  (49 px instead of 65); with every cell hidden, a card is its header alone. Compact nodes stays the
  master switch: while it is on, a card takes one line and the card's switches are greyed, with a
  note saying why. The pipeline editor, and a Run edited for itself, keep detailed cards.
- A Run canvas lifts what lies below a shorter card by as much, from the height of each card's
  visible elements, the way Compact nodes did: nothing is written to the pipeline, no wire gains a
  bend, output labels and conditions stay visible.
- The **Run info tab** block, "Always shown: Name, status, alert banners", shows or hides each
  field of the Run and Stats sections, the read-only note, the Graph rows (Nodes, Edges) of the
  Pipeline section and each extension section, named "Extension · label". By default the
  read-only note, the Status row, Node sessions and the Graph rows are hidden. A section whose
  every element is hidden goes away; the failure and awaiting reasons, the "Reached End" banner,
  the source fetch error and the sandbox preparation always show.
- Their previews are live: the Run canvas block draws a few cards of your most recent Run with the
  canvas's own cards, a fork and a join, at 470 px (zoomed out when the column is narrower); the
  Run info tab block draws your most recent Run's Info tab at 360 px. Both use the most recent Run
  that no other Run started, not an orchestrator's child. With no Run, both show a sample without
  extension sections.
- New seams in the interface: the contributions an Appearance hides are filtered at the `run/info`
  location too. The extension API is unchanged.
- A **Triggers list** block, "Always shown: Name, enable switch, last result", shows or hides on
  every Trigger row the pipeline, the last fire, the next fire, the guard, the overlap policy and
  each `trigger/item` contribution, under "Extensions". By default the last fire and the overlap
  are hidden. With the pipeline, the last fire and the next fire all hidden, Trigger rows take one
  line and the last result moves beside the enable switch.
- A **Pipelines list** block, "Always shown: Name, select control", shows or hides on every
  Pipeline row the version, the draft marker (its change count and who has the draft open), the
  edit date, the validation and each `pipeline/item` contribution, all shown by default. With the
  version, the draft marker, the edit date and every contribution hidden, Pipeline rows take one
  line.
- Their previews use the lists' own rows: the Trigger that fired last, plus a guarded one when it
  has no guard; the Pipeline edited last, plus one with a draft when it has none; or a sample
  without extension marks when you see none.
- New seam in the interface: the `trigger/item` and `pipeline/item` locations filter the
  contributions an Appearance hides too.

## 2.70.0

### Pipeline editor

- A node card lists each of its outputs on a line of its own, with an icon of its port type
  (markdown, html, image, image list, files) and its name; the inputs line is gone. A card with
  several outputs is taller, and the canvas moves what lies under it down by as much, without
  writing anything: positions in the pipeline file stay as they were, a card with one output is
  drawn exactly as before, and no wire gains a bend. The Run canvas is unchanged.
- The node inspector's prompt is a bordered field with a focus ring; the line added at launch
  (`/pdo-interactive`, `/pdo-orchestrate`) sits under it, locked, "Added at launch".
- The Agent section's row is named "Model". In Custom, the summary line no longer repeats the
  three pickers; Inherit and Profile keep it. The Skills selector takes the row's whole width.
- The edge inspector's "When" section is now "Conditions". "Add condition" is always shown
  under the last condition (or under "Always"), and says why when nothing is left to add. The
  field dropdown groups its fields by origin (the output, the loop, the pipeline variables), each
  with its type and allowed values. Output, Field, Op and Value share one grid of same-height
  controls. The "Fields" section is gone. The `when:` key of the file is unchanged.
- Fix: a condition keeps the type of its value when another condition is added or edited. A
  bool condition was saved as the string `"true"`, which never matches a frontmatter `true`, so
  the edge never fired; a string that reads as a number (`"42"`) was saved as a number. An enum
  or string field now always writes its value as a string (an allowed value `"2"` is no longer
  saved as `2`).

### Node detail pane

- The Transcript opens on its latest entries: newest first by default, the Prompt at the oldest
  end. A sort icon in the tab's bar switches to oldest first; the choice is remembered on this
  device and follows you from node to node and Run to Run. Older pages load as you scroll, from
  the end of the transcript. While a node is live, new entries land on top; a reader lower down
  stays in place and a "↑ N new" pill brings them back ("↓ N new" oldest first).
- Frontmatter reads the same in the artifact viewer and the I/O tab: click a value to copy it
  ("✓ Copied" or "Copy failed"), an http(s) value opens with ↗, and a chevron unfolds the
  values cut off to read them whole. Verdicts keep their tone.
- In the I/O tab, each output is a card: its edge in its verdict's tone, its name, path, size and
  tools, and its whole frontmatter, the verdict first, a link or a long value on a line of its
  own. An output not produced yet is dashed and says "not produced
  yet" on hover only. A click anywhere on an output card, a live artifact row or an input row
  opens it; its controls keep their own action.
- A Run search hit inside a long transcript entry scrolls to the marked word, not to the middle
  of the entry.

## 2.69.0 · A shorter dev loop: fast, scoped and full gates, `pdo run status`

### Restarting a node

- A node restarted while the liveness sweep was looking at it stays running: the sweep no longer
  takes the restart's own kill of the old session for a death, so it neither marks the node
  interrupted nor kills the new session.

### Development

- Versions, the CHANGELOG and ADR numbers are set once, when a story merges into `main`: changes
  are written under `## Unreleased`, and a new ADR is born as a draft (`docs/adr/draft-<slug>.md`).
- `make release-bump` proposes the semver level from the conventional commits since the last tag
  (`LEVEL=` forces it), bumps the workspace, the extension API, the shared module of the front
  and `Cargo.lock`, turns `## Unreleased` into the version's entry and numbers the draft ADRs,
  rewriting every citation. `DRY_RUN=1` prints the plan only.
- `make e2e` runs the Playwright suite of the frontend, installing Chromium when it is missing.
- `make fast`, a fast gate that answers in under a minute even on a fresh checkout
  (`cargo fmt --check` and the structural checks, now in the std-only `tidy` crate); `make test`
  lists every failure in one pass (`--no-fail-fast`); the Makefile uses sccache when it is
  installed to share compiled dependencies between worktrees.
- `make scoped` runs `make fast`, then only the checks the diff since the merge-base touches,
  after printing its plan: typecheck, lint and vitest for the frontend, nextest and clippy on a
  changed crate and its dependents, `types-check` for a type exported to the front, nothing more
  for doc. `BASE=` changes the base, `DRY_RUN=1` prints the plan only.
- `make gate`, the gate of a ticket: every step the CI declares except the E2E, plus vitest and the
  script tests, without stopping at a failure; every failure is listed at the end.
- `make demo-daemon PORT=<port>` starts a demo daemon for agentic tests: this branch's `pdo`,
  rebuilt incrementally at each start, a fresh `HOME` under `/tmp/pdo-demo-<port>`, no inherited
  `PDO_*`, `TMUX` or `TMUX_PANE`, copied harness credentials, and a pidfile. `TEST_EXT=1` serves
  the test extension instead. `make demo-daemon-stop PORT=<port>` stops only that daemon and its
  tmux socket `pdo-<port>`, and refuses a port without a pidfile.
- `make bench-gates REF="<ref> …"` measures the dev loop at each ref in a throwaway worktree,
  removed even on failure: cold build, build with a warm sccache, time to the first structural
  failure, the scoped gate on a front-only diff and the full suite, with the machine load; a gate
  the ref lacks reads "n/a". `make run-costs RUN=<run-id>` prints the cost and duration per
  ticket of a Run and its child Runs.
- The `grill-with-docs`, `to-spec` and `to-tickets` skills can be invoked by the model, so the
  chain from grilling to tickets runs without a human command.

### Waiting without polling

- `pdo run status <run-id>` prints the state of a Run in one line: its status, its nodes by state,
  its active children and its cost. `--json` prints the same fields as one compact object. Exit 0
  when the Run is found, 4 when the daemon does not know it, 1 when the daemon is unreachable.
- The seeded `pdo-orchestrate` skill (version 4) waits for children with `pdo run wait` and no
  `--timeout`, as a background task of the harness when it offers one, otherwise with a timeout no
  longer than the tool's; the same rule holds for a build or a long test suite. It reads a Run's
  state with `pdo run status`. No more `run wait --timeout` loop, no `sleep`.

## 2.68.0 · Classic Theme, Dark mode and the acceptance fixes

### Theme and Dark mode

- **Breaking for extensions** (token stylesheet and shared module): `data-theme` changes meaning,
  with no alias. It carries the **Theme** (`acogni` by default, `classic`) and is set on `<html>`
  only; the mode moves to **`data-mode`** (`dark` by default, `light`), on `<html>` or on any
  container. An extension UI that paints with the roles has nothing to change; one that read
  `data-theme="light"` or `data-theme="dark"` now reads `data-mode`. The palette is defined per
  Theme and mode; roles, font, density and radii do not change, and no role token is added.
- **Theme script** served at the stable address `/shared/theme.js`: it sets `data-theme` and
  `data-mode` before the first paint. The interface loads it; a page an extension serves outside
  the interface loads it beside `/shared/tokens.css` (`docs/reference/extension-ui.md`
  § *Tokens*) and keeps no copy of it.
- **`pdo-extension-api` 0.20.0** and **`@pdo/extension-ui` 0.20.0**: the new minor carries the
  break of the token stylesheet, the theme script and the removal of a location from the Rust API
  (see *Node cards*).
- Settings › Interface: **Theme** (Acogni, Classic) and the **Dark mode** switch replace "Theme"
  System / Light / Dark. Classic brings back the colors of the interface before 2.65; the logo
  keeps its ruby. The sun/moon of the top bar only toggles Dark mode; the command field offers
  "Theme: Acogni", "Theme: Classic" and "Dark mode".
- Migration per browser: a `light` or `dark` preference becomes the mode, `system` the system's
  mode at the time of the migration; the Theme is Acogni. The mode is stored under `pdo.ui.mode`,
  the Theme under `pdo.ui.theme`.

### Node cards

- **Breaking for extensions** (`pdo-extension-api` 0.20.0, Rust API): the `canvas/node`
  location is removed, with `UiLocation::CanvasNode` and `CanvasNodeContext`. A node card carries
  no extension decoration any more; show what belongs to a node in its detail (`node/detail`,
  `node/toolbar`, `port/item`) or in the card's menu (`canvas/node/menu`).
- Node cards, in the editor and on a Run: a header tinted by the node's status, with the type
  icon first, in the Theme's accent, and the status icon at the end. An orchestrator shows a
  second sheet behind its card and, on a Run, "N runs" with the Orchestration tab's pills; the
  row opens that tab.
- The whole bar over a child run leads back to the parent run, not only its arrow.
- Edges: an automatic edge goes around the cards between its ends, including one leaving a loop
  towards a card beyond the next, and a condition label slides along its edge instead of shrinking.
  An unplaced condition label keeps off the labels placed by hand on other edges, and off the cards.

### Compact nodes

- Settings › Interface: **Compact nodes**, a switch of this browser (`pdo.ui.compactNodes`), off
  by default, right under Dark mode; the command field offers "Compact nodes" too. On, every Run
  canvas, child runs included, draws its cards on one line: the harness icon, the name, the
  status, and on an orchestrator a second line with "N runs" and its pills. The rest of the
  pipeline rises by as much, computed in the browser: nothing is written to the pipeline, no edge
  gains a bend, side ports fold onto the first line, and output and condition labels stay
  visible. The editor, and a Run edited for itself, keep the detailed cards. A Start, an End or a
  note beside a row of cards holds that row where it is.

### One avatar for a person

- **Breaking for extensions** (shared module): the kit's `Avatar` is round, tinted with the
  person's hue, its initials in ink (first and last word), instead of a neutral square. The hue
  is one of 8 fixed hues (`--av-h0` … `--av-h7`, new tokens with `--av-fill-*` and `--av-solid-*`
  per Theme and mode), none a status color, picked from the person's Actor (`actor`, new prop),
  else its name. New props `hollow`, `present`, `title`. The core draws every person with it: the
  presence on a Run and on a shared draft (whose marks on the canvas take the same hue), the
  Actors, the Review's comments and completions, the versions.
- **Breaking** (shared draft socket): the daemon no longer deals a color to each participant of a
  shared draft; `color` leaves `DraftParticipant` and the `welcome` message.
- New in `@pdo/extension-ui`: `AvatarStack` (three overlapping, then "+n" listing the others),
  `personColor(person)`, the person's color for marks outside its avatar, and
  `useActorDisplays(actors)`, the display names of a list of Actors in one hook.
- A search source result may carry `avatar: { name, actor?, avatar_url? }`, drawn by the core in
  its icon column; a completion suggestion's `avatar` takes `actor` too.
- `Select` takes `active` (a filter in force, tinted); its tooltip is the value shown, no longer
  its label.
- The Pipelines list no longer shows the node count of a pipeline.
- Runs list: a Run archived while it ran ends at its archive (`completed_at`), and its row draws
  its live nodes as stopped; a Retry all keeps the original's name; a row's accessible name is its
  name and status.

### Command field

- **Search results with a fixed status shape**: a search source result's `status` takes an
  optional `shape` (`todo`, `doing`, `review`, `done`), drawn in the tone's color and never
  spinning, so an entity that is no Run no longer spins like a running Run. `StatusIcon` of
  `@pdo/extension-ui` takes the same `shape`.
- **Counter on a global Action**: `UiContribution::with_counter` now also applies to an Action of
  `palette`; its row shows the `PageCounter` the path answers, nothing at 0.
- **Group rank**: `UiContribution::with_rank(n)` on a `palette` contribution orders the extensions'
  groups by rank, lowest first, then those without one, each in registration order.
- A long result title ends in an ellipsis and its facts stay visible.
- **Mark complete** is offered in the command field exactly where the node inspector offers it:
  on a running, awaiting or failed node of a Run that is neither paused nor archived. It is no
  longer offered on an interrupted or stale node, nor in a paused Run, which the daemon refused
  to complete.

### Run inspector and pipeline editing

- `GET /runs/{id}` carries **`variables`**: the values the launch gave the pipeline's variables
  (absent when it gave none). The Variables section of a Run's Pipeline info shows the value the
  Run received, the default only for a variable the launch left out.
- The Run inspector names an iteration by the canvas card's iteration icon and its number
  ("⇄ 2", "⇄ 3/3") in the node header and its picker, the Transcript bar, the enlarged terminal
  and the groups of the Run search.
- Back on a tab, the canvas and the inspector show the selection the tab had.
- The conditions of an edge stay in the order they were added, a new one last, even on a field
  another condition already reads.
- The ages of the Versions list follow the clock; an output's or input's name truncated in the
  inspector is whole in its tooltip.

### Top bar, full-window pages and shared module seams

- A `topbar/end` contribution may be a **panel** (`UiContribution::panel`, `type: "panel"`):
  its icon opens the exported component in a panel anchored under it (`PanelProps`, with
  `close()`). Escape, a click outside and the icon close it. A panel and an action of
  `topbar/end` may carry a counter on their icon, like a page (`with_counter`): absent at 0,
  `99+` past 99.
- A click in a toast no longer closes an open anchored panel (a panel of `topbar/end`, a
  `Popover`, a `Menu`, a `Select`): "Undo" works from inside it.
- A full-window page sets the title and the icon of its header for the route on screen:
  `shell.setTitle(title, icon?)`. A route that sets none, and the page once reopened, show
  the page's label and icon.
- `Banner` takes an `icon`, drawn in the tone's color instead of the status icon (a lock on a
  read-only state).
- The import map resolves `yjs` to the front's own copy (`/shared/yjs.js`): a bundle that lists
  `yjs` as external loads no second copy.

### Launch forms and Triggers

- `pipeline_validation_changed` now also goes out on `/ws` when a pipeline is created,
  duplicated, imported or deleted (a new pipeline is born a test pipeline): a client that keeps
  the pipeline list reads it again, and an open New Run offers the new pipeline at once.
- Triggers name the time zone their cron is read in (UTC): beside the expression in New
  Trigger, in the schedule ("daily at 03:00 UTC") and in the next fire of the Trigger's
  detail, whose tooltip gives the same instant in the browser's time zone.
- New Run says why a repository cannot be used once, under the field: a path that does not
  exist, a folder that is not a repository, a repository without a commit, or one outside the
  Projet. Provisioning is previewed only for a repository the field has accepted.
- Settings: the hover actions of a repository or staging profile row take no room while hidden
  (they always show without a hovering pointer), and under a narrow width the Projects list goes
  above the selected Projet. A path cut in its middle shows an ellipsis in its folders.

### Design system and Workbench

- **`Segmented` takes `fill`** (`@pdo/extension-ui` 0.20): the control takes its row's width,
  its items share it equally and a long label ends in an ellipsis. Without it, nothing changes.
- **A tooltip waits for the pointer to move**: a surface that opens under a still pointer no
  longer shows the tooltip of the control that lands there, and a focus given back by code (a
  menu closed with Escape or picked from) no longer reopens one. Tab and the arrows still do.
- The top bar's Dark mode toggle exposes its state (`aria-pressed`) under the stable name
  "Dark mode".
- An unknown value stored under `pdo.ui.theme` or `pdo.ui.mode` is rewritten as the default,
  as a migrated one is.
- A Workbench layout whose side panels were dragged to their maximum is restored as dragged,
  the canvas keeping its pixels instead of growing back to a third of the window. A drag and a
  restore both leave the canvas at least 5 % of the window.

### Stats, Review, What's new and the artifact viewer

- Stats › Cost: the Name column keeps a readable width at 1024 px and 120 %; below that the
  table scrolls sideways, the Name column held in place and the right edge fading while
  columns remain. A median the line cuts short reads whole in its tooltip.
- Review: a folded thread, and a row of the comments list, keep their line number; only the file
  name gives way.
- What's new reads this changelog in English, one entry per version, its latest ten versions. A
  release title is text, no longer a link. "Release notes offline" only shows when the update
  check is on and its source unreachable; otherwise the banner reads "Update check off", or "No
  release source" when the build declares none.
- The artifact viewer's footer names the Run (its id when it has no name, as in the Runs list);
  the frontmatter keeps the file's order and reads a list as its items.
- A mounted page's tab shows its name in mono beside the book in the accent color.
- Overview tour: the archive step points at the archive icon of the row, and Skip goes once the
  Run is archived.

## 2.67.0 · Session detail bricks, `session/detail` location, page counter

- **`pdo-extension-api` 0.19.0** completed: the `session/detail` location (a tab), the counter
  of a page (`UiContribution::counter`, `with_counter`, `PageCounter`). **Breaking**:
  `Host::live_artifacts` answers `Result<Vec<LiveArtifact>, HostedRefusal>`, `404` for a key the
  extension never gave (an empty list before), `503` without a hosted session.
- `@pdo/extension-ui` 0.19.0 exports the bricks of a hosted session's detail, which the
  extension lays out: `SessionHeader`, `SessionActions`, `SessionWaiting`, `SessionTabs`,
  `LiveArtifacts`, `SessionInfo`, `SessionConfig`, `SessionEvents`, and their live state
  (`useHostedSession`). Types in `pdo-extension-ui.d.ts`.
- New routes of a hosted session by its id, each the `session` Operation of its gesture
  (`read`, `terminate`, `relaunch`, `configure`): `GET /sessions/<id>/hosted` (with `driving`,
  who drives its terminal), `POST …/terminate`, `POST …/relaunch`, `PUT …/config`,
  `GET …/events`, `GET …/live[/<path>]`.
- A hosted session keeps its log (its last 50 changes, and who made them) in its record (an
  older record has none).
- A `topbar/end` page that declares a counter shows it on its icon for the current Actor, read
  again on every event of its extension on `/ws`; `/ws` relays `hosted_session_forgotten`,
  `hosted_live_artifacts_changed` and `extension_event` to the front.

## 2.66.0 · Sessions hosted for an extension

### Open, hold and terminate a session outside any Run

- **`pdo-extension-api` 0.19.0**, non-breaking: an extension has the core hold a hosted session,
  agent or shell, outside any Run, through its `Host`: `open_session`, `hosted_session`,
  `hosted_sessions`, `terminate_session`. New `SessionKind::Hosted`, `SessionRef::extension` and
  `SessionRef::key` (absent for the other sessions, the JSON of their events does not change),
  and the `hosted_session_changed` event, published to the extensions and on `/ws` to the
  connections that may read the session.
- Opening and terminating are the Operations `session` × `open` and `session` × `terminate`, in
  the session's Project; the catalog of Resources lists them. Attaching its terminal is
  `session` × `attach` in the Project of its record.
- The cap of concurrent sessions counts the live hosted sessions with the nodes: the `live` gauge
  of `GET /sessions` includes them, a node waits for a slot they hold, and an opening beyond the
  cap is refused at once (`409`).
- Their records and state live under `~/.pdo/hosted/<id>/`; the working directory is never
  modified. The sweep never kills a hosted session recorded as live, and kills any other name with
  the `pdo-hosted-` prefix.
- An extension can no longer set `PDO_SESSION_ID` in a session: it is a core variable.

### Relaunch, configure, type text, live artifacts, declared wait

- `Host::relaunch_session`, `configure_session`, `type_in_session`, `forget_session`,
  `live_artifacts` and `live_artifact`; `HostedSession::wait_message`; `LiveArtifact`; the events
  `hosted_session_forgotten` and `hosted_live_artifacts_changed`, published like
  `hosted_session_changed`.
- New actions of the `session` Operation, listed in the catalog: `relaunch`, `configure`, `type`,
  `forget` and `wait_user`. An input that lifts the wait of a hosted session asks `session` ×
  `resume` of whoever types it.
- Relaunching resumes an agent's conversation through the identity set at its launch, on the same
  harness; otherwise a new conversation reads the extensions' text and the initial prompt again.
  The resume templates of the `claude`, `copilot` and `pi` harnesses gain a `{model}` hole, empty
  when a node resumes: its command does not change.
- `pdo wait-user` targets the hosted session of `PDO_SESSION_ID` when `PDO_RUN_ID` is absent
  (`POST /sessions/<id>/wait-user`); `pdo complete`, `pdo fail` and `pdo skip` are refused there
  (code 3).
- `PDO_LIVE_DIR` is a core variable in a hosted session: an extension cannot set it. At startup,
  the live hosted sessions of a missing extension are terminated, their record kept.

### Session terminal for extensions and terminal zoom

- `Host::zoom_session`, `HostedSession::terminal_zoom` (100 for an older record) and the bounds
  `MIN_TERMINAL_ZOOM`, `MAX_TERMINAL_ZOOM`, `DEFAULT_TERMINAL_ZOOM`.
- `@pdo/extension-ui` 0.19.0 exports the core's session terminal (`SessionTerminal`), for any
  session by its id, with a zoom set by the host, as well as the terminal zoom control
  (`TerminalZoom`) and its hook (`useTerminalZoom`). Types in `pdo-extension-ui.d.ts`.
- New routes `GET` and `PUT /sessions/<id>/terminal-zoom`; the setting is the Operation `session`
  × `zoom`, listed in the catalog with `session` × `read`.
- On `/ws`, an `operation_applied` on the `session` Resource only goes to the connections that
  may read the session, like `hosted_session_changed`.

## 2.65.0 · Welcome, tours and reading in the Acogni style

- The welcome shows the lockup; the tours are shorter, and their archive step reaches the row
  menu of the tour's own Run.
- What's new names no product, keeps its footer on one line and shows the recent changelog only.
  The messages of the daemon the interface shows name no product either.
- A mounted page shows its actions as icons and its reading guide as numbered entries.
- The artifact viewer shows the frontmatter on one line and pages through files and iterations
  from its header.
- The terminal palette and the harnesses' colors come from the tokens; every remaining screen
  uses the type scale and the roles.

## 2.64.0 · Settings in the shared design system

- Settings categories are aligned rows with no help sentence on screen: explanations go to
  tooltips, a setting reads without hovering, and the search keeps its panels.
- Agent profiles in columns; deleting one is confirmed in the kit's dialog.
- A changed setting is discarded or saved (Discard, Save, Ctrl+S); closing with unsaved changes
  asks for confirmation in the kit's modal.
- Worktree provisioning in rows, aligned groups, saved on close even while the preview is
  pending.
- Extensions: their settings sections sit under "Extensions", each page reached from the rail.
- The kit's segmented control answers Home and End.

## 2.63.0 · Stats and Review in the shared design system

- Stats: the sections become a strip of tabs under the header, the period a segmented control,
  Refresh and Pricing details icon buttons. The extensions' tabs follow Performance with a
  generic icon. Each tab opens on a filter bar (its drill path as a breadcrumb), the figures on
  one line; the tables sort by any column, carry an inline share bar and hold at 1024 px and
  120 %, long names in an ellipsis. Absorptions show a count pill, their members and Combine in
  the kit's dialog frame.
- Review: a header (the Run, its branch, the stats of the pair, the comments) and a bar (file
  list, the pair, viewed count, split / unified, collapse all, the extensions' actions). The file
  tree in numeric order; file cards with "Viewed" (per browser and per pair, `v` on the current
  file); comment threads in a box that folds, a draft and a folded thread kept in their card in
  Split.

## 2.62.0 · New Run, Triggers and Projects in the shared design system

- New Run: choosing the Project, a pipeline combobox, the secondary repositories. Ctrl+Enter
  follows the conditions of the Launch button and Tab stays inside the form.
- Triggers: the guard test and the history of fires as tables; a Trigger's detail in collapsible
  sections.
- Settings › Projects: the Project modal no longer overflows, paths are cut in the middle.
- A dialog takes back the focus it lost; the kit's modal only handles the Escape aimed at it.

## 2.61.0 · Editing a pipeline in the retained style

- Node edit, edge, region, Start/End and Pipeline info as label-left rows; the Agent control is a
  segmented control with its pickers; the tabs fold into an overflow menu.
- The files of a diff are source-control rows; Versions and a Run's Version row.
- A narrow pane keeps the agent, the conditions, the pipeline name, the diff's refs and the
  Custom model readable.

## 2.60.0 · Canvas and Run inspector in the retained style

- Node cards with a grid of facts; rounded edges with bounded condition pills, which never cover
  another; loop regions with their counter.
- A child Run opened from Orchestration lands on the node waiting in it, and its back bar
  returns to the parent.
- Run inspector: tabs and sections, banners on one line, a dark terminal outside the interface
  zoom (its font carries the scale), I/O rows with the verdict and a counter of the other keys.
- Transcript as message cards with folded tool calls; live artifacts as rows under the Outputs
  with a live dot; Orchestration in two-level rows, tinted on failure or wait.
- The canvas context menu uses the kit's style.

## 2.59.0 · Lists of the left panel on two lines

- Runs on two lines of 46 px: the status mark that selects, the name, the Trigger, the child
  counts and the node progress; then the pipeline, the extensions' marks and people, duration
  and cost. Archived Runs are dimmed; a hover action and the "⋯" menu sit in a floating bar,
  also on keyboard focus.
- Project groups: a caps header, a count kept while folded, an awaiting dot.
- Triggers: the switch first, mode icons (guard, overlap), the last fire and the next one.
- Pipelines: a mono name, the lint only when invalid, validation per Project, version, draft and
  edit date.
- A floating bar acts on a selection; an empty list shows an icon, two words and one button.
- `GET /runs` carries the progress of each Run's working nodes and `completed_at`;
  `GET /runs/costs?ids=` prices the listed Runs apart; a pipeline that does not parse says why
  (`invalid`).

## 2.58.0 · Command field: palette, Actions and search sources

- **`pdo-extension-api` 0.18.0**, non-breaking: the `palette` location (the command field,
  Ctrl+K) accepts global Actions and the new search source contribution type
  (`UiContribution::search`, `type: "search"`), whose function answers the queries typed (at most
  50 results kept, abandoned after 5 s, isolated in its own group on error). A command can
  declare the Operation it performs (`UiCommand::guarded_by`, `operation` on the wire): the core
  then hides, everywhere, the Action that the Operation authorization denies. The shared module
  `@pdo/extension-ui` moves to 0.18.0 (types `SearchResult`, `SearchFunction`,
  `PaletteContext`). Details in `docs/reference/extension-ui.md`.
- The palette remembers the five most recently opened items (`pdo.palette.recent`, per browser)
  and offers them on an empty query.

## 2.57.0 · Unambiguous pipeline Save

- Saving a pipeline offers three exclusive outcomes: a new version, a new version keeping the old
  one aside as the pipeline `<name> (vN)`, or replacing the latest version (same number). The
  "replace and keep" combination is gone.
- Shared kit: a key (`Kbd`) inside a button takes the button's color.

## 2.56.0 · Run filters in a funnel

- The `runs/filter` and `projects/filter` locations move, without any API change: their
  contributions are rows of the funnel menu in the Runs header, after the core's filters. A set
  attribute counts as an active filter and "Clear all" empties it. `runs/header` is placed
  before the funnel and "New Run". Details in `docs/reference/extension-ui.md`.
- The "Project" button of the Runs header is removed: Projects are managed in Settings.

## 2.55.0 · Shell Workbench

- The shell's extension locations move, without any API change: `topbar/end` goes before the
  core icons (New run, Stats, Settings, the theme), `topbar/account` after the theme, and
  `statusbar` fits on one 22 px line (icon and number, the name in a tooltip). Details in
  `docs/reference/extension-ui.md`.
- The left panel opens at 260 px and the inspector at 400 px, whatever the window size. A layout
  already saved (`pdo.layout.run`) keeps its percentages.
- The "Run now" error becomes a toast, which disappears like the others.

## 2.54.0 · Acogni Driver: shared design system, embedded fonts, UI scale

- **Breaking for extensions**: all style tokens are renamed, with no aliases (the decision
  record explains why). Colors go through roles (`--surface-0/1/2`, `--text`, `--text-2`,
  `--text-3`, `--line*`, `--primary*`, `--hover`, `--selected`...) and an OKLCH palette for both
  themes (`--color-paper*`, `--color-ink*`, `--color-st-*`); `--color-bg-*`, `--color-fg-*`,
  `--color-acc*`, the `--color-st-*-bg` and `--font-sans`/`--font-mono` are removed. The mapping
  table is in `docs/reference/extension-ui.md` § *The design system*. On the core side, the
  Tailwind utilities follow the roles (`bg-surface-1`, `text-text-3`, `bg-st-failed/14`) and the
  type scale replaces Tailwind's (`text-2xs` to `text-xl`, 10 to 16 px).
- **`pdo-extension-api` 0.17.0**, non-breaking for the Rust API: the shared module
  `@pdo/extension-ui` exports the design system primitives (`Button`, `IconButton`, `Select`,
  `Menu`, `Modal`, `ConfirmDialog`, `toast`, `Tabs`, `Section`...), types included. The token
  stylesheet is served at the stable address `/shared/tokens.css`.
- Archivo and Geist Mono are served by the binary (`/shared/fonts/`, SIL OFL 1.1 licenses): the
  interface no longer calls any CDN.
- "UI scale" setting (Settings › Interface: 100, 110 or 120%, 120 by default), per browser
  (`pdo.ui.scale`).

## 2.53.0 · Clone a remote repository from Settings › Projects and New Run

- "Add repository" of a Project offers "Clone from a URL...": the repository is fully cloned into
  the clones folder, with its progress and a Cancel, then joins the Project. The clone continues
  with Settings closed. New Run also accepts a URL: "Clone and launch" clones then launches the
  Run on the clone; a URL already cloned is reused ("Launch").
- New routes `POST`/`GET /repos/clones`, `GET`/`DELETE /repos/clones/{id}` (cancels a running
  clone, forgets a finished one) and `GET /repos/clone-target`. The clone is made in a hidden
  neighboring folder, renamed once complete: a failure or a cancellation leaves nothing behind.
- New instance setting `clones_folder` (default `~/pdo/repos`), "Clones folder" section of
  Settings › Projects.

## 2.52.0 · The library assistant and external writes go through the draft

- The library assistant no longer writes a file: `POST /sessions/libassist/save` applies to the
  shared draft of the focused pipeline, like a participant of its Actor, and answers `draft`,
  `fields` (the changed fields) and `version`. It reads this draft through
  `GET /sessions/libassist/draft`. Only a pipeline whose file cannot be read, and which
  therefore has no draft, is still saved, as a version signed by the Actor.
- A file written outside PDO becomes an "external edit" version (no author) before merging into
  the draft; the `/ws` stream marks its `pipeline_changed` with `outside: true` and the draft
  socket sends a `notice` to the participants.

## 2.51.0 · Settings › Projects

- New "Projects" category in Settings: create, rename and delete a Project, add a repository
  (recent, local folder, new empty repository), remove it, choose the default primary repository
  that New Run preselects. Changes show live in the other browsers.
- `GET /repos/remote?path=` returns the remote of a repository, without its credentials (`null`
  without a remote). `GET /projects/deleted` returns the deleted Projects (id and last name),
  kept in the `deleted_projects` table so that their Runs still name them ("<name> (deleted)").
- `POST /repos/create` accepts an empty folder and makes the first commit even without a file.
- The refusals of `/repos/validate` and of Run creation name the folder rather than the
  `target_repo` field: `<path> is not a git repository`, `<path> does not exist or is not a
  directory`, `repository must be an absolute path: <path>`.

## 2.50.0 · Runs list: date filter, collapsing a Project

- **Run summary v4**: it gains the instant of the last activity and the activity ranges (UTC
  instants; two events less than an hour apart fit in the same range). Summaries are rebuilt when
  the daemon starts. `GET /runs` carries `last_activity_at` and `activity` on each entry.
- Run filters: Date axis, Active (the default, any activity in the range) or Started, with
  presets and a free range; it also applies to archived Runs. The filter does not survive a
  reload.
- A Project group collapses; the collapsed state is kept per browser
  (`pdo.ui.collapsedRunGroups` in the local storage).

## 2.49.0 · Live artifacts

- **Pipeline format**: the intermediate output becomes the live artifact (separate from
  outputs). An `agent` or `script` node gains `live_artifacts` (`name`, `port_type`,
  `instructions`, `frontmatter`) and `free_live_artifacts` (the node may publish undeclared
  artifacts; the editor turns it on for a new node, a silent document leaves it off). Both keys
  are refused on a structural node.
- **Migration on load**: an `intermediate: true` output of an `agent`/`script` node is read as a
  declared live artifact (name, type, expected content, schema); the file is rewritten on save.
  Elsewhere, the key is ignored with a warning. A node library entry is read the same way.
- **Refused at parse**: an edge that carries a live artifact, a live artifact name that is not a
  plain file name or that is declared twice, an output named `live` on a node that publishes live
  artifacts.
- The live artifact is published in `<artifacts>/<node>/iter-<n>/live/`: `<name>.md`,
  `<name>.html`, or the folder `<name>/` of its images or files (type `files`). A `script`
  receives `PDO_LIVE_DIR`.
- `GET /runs/{id}/nodes/{node}/io` loses `intermediate` on the ports and gains `live`: the
  declared then free artifacts, with their files, `schema_warnings` and `updated_at`. The
  `PortBrief` of a Run's `node_defs` loses `intermediate`.

## 2.48.0 · Pipeline versions

- Save freezes the shared draft into a version, numbered and titled (`vN` by default): a "Save..."
  window with "Save as a new version" (unchecked, the latest version is replaced under its
  number) and "Keep old as new pipeline" (the version saved until then becomes the pipeline
  "<name> (vN)"). The registry file remains the latest version: New Run, the Triggers and the
  export read it. Versions live in the SQLite table `pipeline_version`.
- **Migration**: a pipeline without a version becomes v1 on its first read, titled by the free
  `version:` field of its YAML (otherwise `v1`). This field disappears from the Identity of
  Pipeline info; the YAML keeps it.
- **No more silent saving at launch**: New Run no longer writes the modified tabs. A draft that
  differs from the latest version asks "Save as vN and run" (default) or "Run last version";
  creating or editing a Trigger no longer saves anything.
- History: "Versions" tab of Pipeline info (draft, versions, author, date, number of Runs);
  "View" opens a version read-only, "Restore" puts it back into the draft for everyone, in a
  single Undo, without creating a version.
- A Run remembers its version (`pipeline_version` of `RunStarted` and of the Run state); its Info
  shows "vN · title" and "vM is out" when a more recent version exists.
- API: `GET /pipelines/{id}/versions`, `GET /pipelines/{id}/versions/{n}`; `PUT /pipelines/{id}`
  takes `version` (`new_version`, `title`, `keep_old`) and answers `version` and `kept`;
  `GET /pipelines` gains `version`; the `/ws` stream announces `pipeline_version_changed`. Run
  summaries (version 4) are rebuilt at boot.

## 2.47.0 · Launch in a Project; a repository in several Projects

- **Breaking.** A Run's Project is chosen at launch and recorded on the Run, never deduced from
  its repository. `POST /runs` and `POST|PATCH /triggers` accept `project_id`; a named Project
  must exist and contain the primary repository (`400` otherwise). Without `project_id`, a Run is
  **outside any Project**, even on a repository that is a member of a Project; a Run created from
  a node session inherits the Project of its parent Run, a Trigger launches its Runs in its
  Project. The `Operation.projects` of a Run, of its nodes, sessions and Review comments are this
  recorded Project: an extension that read the repository's Project must read this field
  instead.
- A repository can belong to several Projects: no more `409` "already in another Project".
  `DELETE /projects/{id}/members` removes the repository from this Project only. Each Project
  designates a default primary repository (`primary_repo`, `PATCH /projects/{id}`), by default
  its first member.
- **One-time migration at startup.** Each existing Run receives in its `run_started` the Project
  that the old rule assigned to it (`project_id`, `null` if it had none), then the Run summaries
  are rebuilt; each Trigger likewise receives the Project of its target repository. A Run
  already recorded is never revisited: changing the members of a Project moves no Run.
- `GET /runs` and `GET /runs/{id}` carry `project_id`; the Runs and Triggers lists are grouped by
  recorded Project. Stats file a Run's cost under this Project.
- **`pdo-extension-api` 0.16.0**, non-breaking: `NewRun::in_project(id)` launches the Run of a
  `Host::create_run` in a Project. Without it, the Run is outside any Project.
- `pdo run create --project <id>` launches in a Project; from a node session, the child Run stays
  in its parent's Project.
- New Run › Where starts with the Project (the last one chosen is preselected, "No project" only
  if the Rights allow it), preselects its default primary repository and offers its other
  repositories as checkable secondaries, read-only by default.

## 2.46.0 · Files output type, artifact download

- New output type `files` (`PortType::Files`): one or more files, in any format, in
  `<node>/iter-<N>/<port>/`. Completion refuses a folder without a file (`missing_outputs`, a
  subfolder does not count). Downstream receives the port's folder, in the preamble, in the
  `PDO_INPUT_*` of a script and in the I/O panel; an accumulation gives one folder per completed
  iteration.
- `GET /runs/{id}/artifact?path=...&download=true` returns the raw bytes as an attachment, with a
  readable name (`<node>-<port>.md`, suffixed `-iter-<N>` after the first iteration).
  `GET /runs/{id}/artifact/zip?path=...` returns the zip of a port folder (`<node>-<port>.zip`).
  Both routes share the traversal guard and set `nosniff`; never `text/html` (decision record on
  artifact serving).
- Run view: "files" badge, one chip per file that downloads it, "Download all" (zip), a down
  arrow icon on hover of any artifact row and in the header of the markdown modal. The modal of
  a files port paginates its files and displays them according to their format (image, text, or
  download).
- Inspector: "Files" option in the output type selector.

## 2.45.0 · Total or Active duration of a node

- The iteration state exposes `waited_ms` (closed declared waits) and `open_wait_since` (wait in
  progress). Both fields are absent from the wire when empty. The rule for declared waits
  (`pdo_run::domain::declared_wait`) is shared by the projection, Stats › Performance and the
  lifecycle telemetry.
- The meta line of a node's panel shows the iteration duration, which ticks while it runs. A
  click (or Enter, Space) toggles between Total and Active; the choice is remembered
  (`pdo.ui.nodeDurationMode`) and also applies to the iteration menus. Active stays frozen during
  a declared wait and Total = Active + Waiting at the displayed second. The tooltip gives the
  start, the end and the Total / Active / Waiting breakdown.
- The iterations menu puts the most recent first.

## 2.44.0 · Canvas: edge grabbable at any zoom, validated output rename

- A node's edge is grabbable at any zoom: 8 px on screen, of which at most 3 px (and 25% of the
  card) inside. The edge starts from the pressed pixel. Zoom out down to 25%, zoom displayed at
  the bottom left of the canvas.
- Snapping follows the zoom: pipeline step x 2, 4, 8... as long as a cell is less than 16 px on
  screen, "snap x4 · 120px" badge at the cursor. The saved layout does not depend on the zoom.
- Renaming an output applies on Enter or when leaving the field, Escape cancels it. A taken or
  empty name is refused. Edges follow and the downstream emergent input takes the new name.
- An edge to a missing port is shown in dotted red, "<port> · port missing", and can be deleted
  directly. After saving, the lint is read again.
- Outside a Run, the inspector no longer has a "Run" tab.

## 2.43.0 · Shared pipeline draft and presence

- A registry pipeline is edited in a shared draft: one Yjs document per pipeline, held by the
  daemon (SQLite) and synchronized through `WS /pipelines/{id}/draft`. Changes merge at field
  granularity, prompts at character granularity. The draft survives a daemon restart; Save
  writes it to the file and resets it.
- Presence: avatars in the tab bar and on the row of the Pipelines list, other people's selection
  on the canvas (outline and label in their color), typing in a prompt (named caret, "X is
  typing"). `GET /pipelines` gains `draft` (`changes`, `nodes`): the row shows "draft: N
  changes" and counts the draft's nodes.
- Undo only undoes its own changes, and its tooltip names the last one.
- An Operation authorization that denies `pipeline × update` makes the user join the draft
  read-only.
- A file written outside PDO merges into the draft: the conflict window is gone.

## 2.42.0 · Review file tree

- The Review file list is a tree of collapsible folders. A chain of single-child folders fits on
  one line, each folder shows the count of its unresolved comments, and a button next to the
  counter collapses or expands everything.
- The diff cards follow the tree order (folders before files).
- Keyboard: Left/Right arrows collapse and expand the folder of the current file, j/k skip the
  files of a collapsed folder. The "/" filter keeps the tree and opens the branches that contain
  a match.

## 2.41.0 · Pipeline validated for everyone and listed first for Projects

- The `all` validation of a pipeline accepts a list of Projects:
  `{"kind":"all","project_ids":[...]}`. `{"kind":"all"}` remains valid (empty list), with no
  migration. An unknown Project is refused (`400`), a duplicate is collapsed.
- In New Run, these Projects see the pipeline under "Project pipelines", the others under
  "Global pipelines". Deleting a Project removes it from the list and leaves the pipeline offered
  to everyone.
- Validate modal: under "All projects", the Project checkboxes stay active ("Listed first for");
  the verdict fits in one sentence.

## 2.40.0 · Reading guide of a mounted page, Public URL

- `pdo page mount <name> <directory> --guide <file>` attaches a reading guide (YAML or JSON, at
  most 50 entries). The daemon validates it before mounting: a refused guide mounts nothing and
  the command says why. The guide is stored with the mount and leaves with it (`unmount`, archive
  of the Run that mounted the page).
- The guided page opens in a tab of the central panel, the guide in the right panel. A guide link
  leads to a Run, a node, an artifact, a Review file (`/runs/<run>/review?file=`), an extension
  page or an anchor of the page.
- New **Public URL** setting (Settings › General, or `PDO_PUBLIC_URL`; the stored setting wins):
  an origin `http(s)://host[:port]`. `PUT /settings` gains `public_url` (`""` clears it). A node
  session receives it in `PDO_PUBLIC_URL`, `pdo page mount` prints its addresses under it, and
  Settings › Telemetry names the address of `/metrics` with it.
- A Review `?file=` link brings you to the file even below the fold.

## 2.39.0 · Skill completion in prompts, skill export and upload

- `/` at the start of a word in the prompt of a node or of New Run opens the skills list at the
  cursor; choosing a skill that the node (or the Run) does not receive adds it to its tier.
- `POST /settings/skills/export` serves a zip in the sidecar format, named after the folder, the
  skill or `selection` for a mixed selection.
- `POST /settings/skills/upload?name=` imports a zip according to the sidecar's import policy:
  new skills arrive in a folder named after the zip, flat (the archive's hierarchy is not
  kept). The Operation `skill.import` is requested before reading the archive.

## 2.38.0 · Search in the Run transcripts

- `GET /runs/{id}/transcripts/search?q=&client=` searches the transcript of each node iteration
  that PDO reads: message text and the one-line summary of tool calls, never the model's
  thinking. Results by node (pipeline order) then chronologically, capped at 200 excerpts, all
  counted per node. The next search from the same `client` cancels the previous one on the server
  side (response `cancelled: true`), as does closing the connection.
- No persistent index: the 1st search on a very large transcript pays for its in-memory
  indexing, the following ones only reread what the file has gained.
- Matching is case-insensitive and accent-sensitive (a query without the accent does not find the accented word).
- Benchmark: `make bench-transcript-search`.

## 2.37.0 · Prometheus `/metrics` and "include the Actor"

- Settings › Telemetry gains the "Prometheus /metrics" section: `GET /metrics` serves the same
  metrics as the OTLP export in Prometheus text format (no traces), with its own switch, **off by
  default** (`404` while it is). The route requires nothing more than the rest of the daemon:
  whoever reaches the instance reads it. Setting `telemetry_prometheus`, env
  `PDO_TELEMETRY_PROMETHEUS`.
- "Identity" section: "Include the Actor", **off by default** (`telemetry_include_actor`, env
  `PDO_TELEMETRY_INCLUDE_ACTOR`). When on, `pdo.actor` (who launched the Run) is added to the Run
  and node metrics and to the spans, on both paths, and the answer to a declared wait becomes a
  `pdo.wait.answered` event of the node's span. An agent is named `agent`, without its ids. When
  checked, a warning recalls prior information of the people concerned, consultation of the works
  council and the GDPR basis before saving.
- The `NodeResumed` (Enter typed in the terminal) and `NodeCompletionReleased` (node button)
  events now carry the Actor who answered (`payload.actor`). Older events have none and read as
  before.
- `GET /telemetry/status` also says `metrics_endpoint` and `include_actor`; "What is sent" lists
  `with_actor`. All changes take effect without a restart.

## 2.36.0 · Transcript tab of a node

- `GET /runs/{id}/nodes/{node}/transcript?iter=&after=&limit=` serves the normalized transcript
  of an iteration, in pages (Prompt / Agent messages, tool calls collapsed);
  `GET .../transcript/tools/{index}?iter=` serves the input and output of a call. A call that
  replaces text carries a diff (`keep` / `remove` / `add`) and its `+N -M` count.
- The model's thinking is dropped at read time: it appears neither in the pages nor in the detail
  of a call.
- A harness without a transcript reader answers `unavailable`; the capability can be read in the
  support table ("Transcript tab" row).

## 2.35.0 · OTLP export of metrics and traces

- New "Telemetry" category of Settings: the daemon pushes its metrics and traces over OTLP/HTTP
  to a collector. **Off by default**: nothing connects until the switch is turned on.
  `PUT /settings` gains the switch, the address and the headers of the collector; a header's
  value is never returned (`GET /settings` only says it is set). A change reloads the export
  without a restart.
- `GET /telemetry/status` and `POST /telemetry/test-export`, authorized like a change to the
  instance settings.
- Attributes name the repository by its remote without credentials, never by a local path, and
  never carry the Actor. `service.instance.id` is `<host>:<port>`.
- An unreachable collector is logged once, reported in the status, with no effect on Runs; traces
  emitted during the outage are lost.

## 2.34.0 · Text sending reserved to the pilot, refreshing a harness's models

- **`pdo-extension-api` 0.15.0**, non-breaking: the `instance_settings` operation gains the verb
  `refresh_catalogue` (re-probe a harness's models). An extension that answers `authorize` can
  deny it like `update`.
- `POST /runs/<run>/nodes/<node>/terminal-text` accepts `poste` (the sending station). A station
  watching a node's terminal while another one pilots it receives `409 not_pilot` (with the
  pilot's Actor), and nothing is pasted. A send without a station (CLI, extension) goes through
  as before.
- `POST /settings/harnesses/{name}/refresh` re-probes a harness's catalogue even at an equal
  version, and `/ws` announces `harness_catalogue_changed`. The served catalogue carries
  `probed_at`.

## 2.33.0 · Ports without side or `repeated`

- **Pipeline format change.** A port no longer has a side (`side`) or a `repeated`: the inspector
  of an output no longer offers `L/T/R/B` or `rpt`, and its type moves into the name line. Where
  a wire starts belongs to the edge, and so does accumulation. On load, an output's `side`
  becomes a `source_anchor` without `offset` (middle of the side) on its edges, an End's `result`
  one becomes a `target_side`, and a port's `repeated` is ignored; the node-end check no longer
  looks for an output in previous iterations (a running Run whose `repeated` output was only
  written in an earlier iteration must rewrite it to finish the current iteration). The file is
  only rewritten on save: run `pdo migrate` to convert the registry (`~/.pdo/pipelines`) and the
  node library (`~/.pdo/library`); the command says, file by file, what it changed, and a second
  pass changes nothing.
- `EdgeAnchor.offset` becomes optional; `PortDef` loses `side` and `repeated`, `PortBrief` loses
  `side`. The `side` of a `PortBrief` from an earlier event log is ignored on read. Minor bump:
  every file and every Run written before can be read again without error.
- The editor now writes an edge's `repeated`: a save from the canvas used to lose it.

## 2.32.0 · Actor author of Review comments, composer autocomplete sources

- **`pdo-extension-api` 0.14.0**: new UI location `review/composer` (the Review comment composer)
  and new contribution kind `completion` (`UiContribution::completion`, `.rendering`): a trigger
  character, a suggestions function exported by the bundle, the inserted text, and optionally the
  rendering of this text in the comment. Without a contribution, the composer does not change.
  Reference: `docs/reference/extension-ui.md`.
- A Review comment, each of its replies, its resolution and its reopening record the request's
  Actor (`actor`, `resolved_by_actor`, `reopened_by_actor`), in addition to the role (`author`,
  `by`). The `review_comment_*` events carry it, with the text. Comments written before remain
  readable, without an Actor. When an extension identifies the caller of an agent session, the
  role (`manager`, node id) can be read in the session headers.

## 2.31.0 · Agent sessions and the Actor who starts them

- **Extension API 0.13.0**, non-breaking: hooks `session_env` and `session_preamble`, requested
  for any session that makes an agent act (node, Run Manager, library assistant) with the Actor
  who starts it; by default, `node_env` and `node_preamble` for a node and nothing for the
  others. Events `session_started`, `session_taken_over` and `session_ended` (`SessionEvent`), on
  `/ws` for a Run's session, once an extension is loaded.
- **One library assistant per Actor**: `POST /sessions/libassist` opens the caller's; the local
  operator keeps `pdo-libassist-shared`. Focus, save and close are the caller's.
- **Opening the assistant changes Operation**: `pipeline` × `open_assistant` (no target, outside
  any Project) replaces `session` × `create`. A rights extension that denied the assistant
  through `session` × `create` must deny the new Operation.

## 2.30.0 · A Run seen by an extension: people, Presences, cost

- **The core no longer draws a Run's creator**: neither on its row in the Runs list, nor in its
  Info tab (the "Created by" chip is gone). Without an extension, no person appears. `created_by`
  remains on the API and in the `run/info` context.
- **`pdo-extension-api` 0.12.0**, by addition: the `run/item/people` location (a Decoration on a
  Run's row, with its live Presences), `Host::presences` and `Host::run_cost` (the Info tab's
  cost). An extension written for 0.11 compiles unchanged.
- **Presences**: each tab says on `/ws` which Run it shows (`{"type": "show_run", "run_id"}`);
  the core announces `presence_arrived` and `presence_left` (filtered by the Run's read access)
  and serves `GET /runs/presences`.

## 2.29.0 · Intermediate output

- **Intermediate output**: an output port can be checked "intermediate" (the port's field
  `intermediate: true`, omitted when false). The agent fills it and updates it while working, the
  preamble lists it separately with its instruction, and the node panel row follows it during the
  Run. Writing this file emits no event: the row updates on the next refresh, at most 30 s later.
  `pdo complete` never checks it, a skip writes nothing for it.
- **New refusal at parse**: a Pipeline with an edge carrying an intermediate output is refused,
  naming the port and the edge. A Pipeline without the option does not change.
- The response of `GET /runs/{id}/nodes/{node}/io` carries `intermediate` on each port, and the
  `PortBrief` of a Run's `node_defs` carries it when true.

## 2.28.0 · On-demand node, forced skipped node, waiting for started nodes

- **On-demand node**: option `on_demand` of an `agent` or `script` node (refused on a structural
  node), carried by the library, the portable export and the Run snapshot. The runtime neither
  launches nor skips it; its play button (grayed card, node panel) goes through `node` × `start`
  and reopens a finished Run.
- **Pipeline option `wait_for_started_nodes`** (absent when `false`, the default): a Run that
  reaches `End` stays `running` as long as a node is `running`, `waiting` or `awaiting_user`,
  whether it must end or stop there (edge to `End` with `reason:`). The completion or stop of
  the last of these nodes ends the Run. The Run freezes the option at launch (field
  `wait_for_started_nodes` of `GET /runs/{id}`). For a Run that has it, `GET /runs/{id}` and
  `GET /runs` carry `waiting_for_started_nodes` (number of awaited nodes); the Run summary moves
  to version 2 and is rebuilt at startup. Without the option, a Run reopened by a play button
  ends again when its node completes, without a waiting banner.
- **A finished Run marks the untaken branches `Skipped`**, instead of leaving them pending: they
  thus keep their play button (Retry, `node` × `retry`).
- The preamble of a node launched before its upstream says the input is absent, and a script's
  `PDO_INPUT_<PORT>` is empty, instead of giving a path to a file that does not exist.
- A second `RunCompleted` on an already finished Run is no longer written.

## 2.27.0 · Pipelines validated per Project

`pdo-extension-api` stays at **0.11.0**: the `pipeline` Resource gains the verb `validate`.
Detail: `docs/reference/extension-api.md` ("The catalogue of the Resources").

- **Pipelines created, duplicated or imported after the update are test pipelines** as long as
  nobody validates them: New Run and the Trigger form only offer them behind "Show test
  pipelines". Existing Pipelines remain offered everywhere (no recorded validation = all
  Projects). A rename keeps the validation, a deletion forgets it, an export does not carry it.
- Deleting a Project removes it from the validations; a Pipeline validated for that Project only
  becomes a test Pipeline.
- API: `GET /pipelines` exposes `validation` (`{"kind":"all"}`, `{"kind":"none"}` or
  `{"kind":"projects","project_ids":[...]}`); `PUT /pipelines/{id}/validation` replaces it (`404`
  unknown Pipeline, `400` unknown Project) and broadcasts `pipeline_validation_changed`. The
  filter is the forms' one: Run creation accepts any Pipeline.
- **New verb to map** for a Rights extension: `pipeline` × `validate`. An extension that does
  not know it and denies unknown verbs now denies the Validate button.
- Tutorials: "Validate the pipeline" step in *First pipeline*; *First run* creates or reuses the
  "Tutorial" Project, files the tutorial repository in it and validates `tutorial-interactive`
  for it alone.
## 2.26.0 · Interface up to date after an absence

`pdo-extension-api` and `@pdo/extension-ui` stay at **0.11.0**, a version not yet published
on `main`: `openProbedSocket` gets the `silenceMs` option there, an addition only.

- **Reload after an absence**: each time the `/ws` stream (re)opens, when the tab is focused again,
  when the network comes back, and on the `{"type":"resync"}` frame that the daemon sends to a client
  that lost events, the interface reloads Runs, the open Run, sessions, Triggers and their pause,
  Projects, pipelines and the permitted gestures. Nothing is replayed.
- **Node terminal**: the daemon beats every 10 s on its socket (Ping + `heartbeat` frame).
  After 30 s without receiving anything, when offline, or on close, the terminal is veiled
  ("Terminal connection closed", **Reconnect** button) and reattaches by itself in the same xterm.
  Behind a reverse proxy, its read timeout must exceed the beat: see the WebSockets section of
  the guide.
- **Seeded skill `pdo-interactive` v2**: the `pdo wait-user` banner is a notification; the
  full question or result is written in the conversation before the call.

## 2.25.0 · Polish of the Run list

`pdo-extension-api` and `@pdo/extension-ui` stay at **0.11.0**, a version not yet published
on `main`: it gets the `runs/empty` location and `copyToClipboard`, additions only.

- **Empty Run list**: the `runs/empty` UI location (decorations only) is displayed under
  "No runs yet" as long as the list contains no Run. The core text does not change.
- **Copy outside HTTPS**: `copyToClipboard(text)` from `@pdo/extension-ui` is the copy used by all
  the Copy buttons of the core; on an instance served over http, it falls back to
  `execCommand("copy")`. It never rejects and returns a boolean.
- **Archived tree**: the archived section arranges Runs under their parent, like the active
  one; the "Child runs" preference and the global button apply to both. A Run whose
  parent is in the other section is a root there and carries a link to it. Archiving a parent
  always takes its finished children with it and is still refused while a child is alive: this case
  (active child under an archived parent) does not occur from the UI.

## 2.24.0 · Gestures according to Rights, stream guarded by read

`pdo-extension-api` and `@pdo/extension-ui` stay at **0.11.0**, a version not yet published
on `main`: it gets `ExtensionEvent::guarded_by`, an addition only.

- **Guarded stream for extensions**: `ExtensionEvent::guarded_by(read)` attaches to an
  event the read Operation that protects it; `/ws` only sends it to connections whose
  Actor can perform it, and sends it without the guard to no one. An `operation_applied` on an extension Resource is
  guarded by the read of its entity, in the Projects of its Operation: an extension that
  refused every Operation whatever its verb now also hides, on `/ws`, the
  `operation_applied` of its Resources.
- **Gestures according to Rights**: the Run view calls `POST /authorize` once when it opens
  for all the gestures of the Run and its nodes, and hides those that are refused; the
  rows of the Run list, their menu and the bulk actions follow the same rule. Reloaded when
  a Project changes or when an extension signals a change of Triggers. A gesture that the
  daemon refuses anyway (a Right changed in the meantime) now says so in a toast, where it said nothing before.

## 2.23.0 · File imported during a Run

`pdo-extension-api` and `@pdo/extension-ui` stay at **0.11.0**, a version not yet published
on `main`: it gets two more verbs, additions only.

- **File imported during a Run**: two new Operations on the `node` Resource,
  `import_files` and `send_text` (Resource catalogue). A Rights extension that
  enumerates the verbs of a Node must map them; otherwise they follow its default rule.
  The import counts against the same `max_attachments_mb` budget as the files attached at the launch
  of a Run, per import: behind a reverse proxy, `client_max_body_size` must follow it.

## 2.22.0 · Delivery signed by the git identity of the Node

`pdo-extension-api` stays at **0.11.0**: only the documentation of `node_env` changes. Detail:
the decision record and `docs/reference/extension-api.md` ("Git identity").

- The variables `GIT_AUTHOR_NAME`, `GIT_AUTHOR_EMAIL`, `GIT_COMMITTER_NAME` and
  `GIT_COMMITTER_EMAIL` that an extension gives to a Node are frozen on its `NodeStarted`
  (`git_identity`, these four only) and sign all its delivery: the commit
  `<node> iter-<N>: completed`, the commit of the sub-worktree of an isolated Node, its merge and the
  resolution commit in favor of the Node.
- A daemon without a git identity now delivers, provided all four are given; a missing or empty
  variable falls back to the daemon identity. Without them, nothing changes.

## 2.21.0 · Read-only driving of a Run

- **"Save" in a Run no longer writes the shared pipeline**:
  `PUT /runs/{id}/pipeline` only modifies the snapshot of the Run, neither the shared pipeline nor the
  other Runs. A Run tab opens read-only; the pencil in the bar offers "Edit
  for this run" and "Edit source pipeline". To make a change permanent: "Save > Overwrite default
  pipeline...", which warns (future Runs, Triggers that reference the pipeline,
  shared pipeline modified since the launch) then copies the whole snapshot, orphan prompts
  removed. API: `GET /runs/{id}/pipeline/overwrite-preview`,
  `POST /runs/{id}/pipeline/overwrite-default`. Overwriting requires `run` × `edit_pipeline`
  **and** `pipeline` × `update`: a Rights extension has no new verb to map.
  A Run launched before the update has no launch fingerprint: the warning says it
  cannot tell whether the shared pipeline has changed.

## 2.20.0

`pdo-extension-api` goes to **0.11.0**, and the `@pdo/extension-ui` module with it: additions
only, an extension 0.1 to 0.10 compiles and runs as is. Detail:
`docs/reference/extension-api.md`.

- **Name of a Project**: `Host::project_name(id)` gives the name of a Project, or nothing
  for an unknown id, synchronously (including from `authorize`). The daemon reads the
  Projects before serving and rereads a Project on each change, before announcing it.
- **Live Triggers**: the interface stream relays `trigger_updated` and
  `project_changed`, which it used to drop; a Trigger modified or a Project renamed elsewhere
  updates without reloading. `Host::triggers_changed()` announces a `trigger_updated` without
  `trigger_id`, and the Trigger list rereads everything.
- **Triggers read in their Project**: the Operations on a Trigger are in the
  Project of its target repository (none without a target repository). `GET /triggers`, `GET /triggers/{id}`,
  its firings and what `/ws` says about a Trigger require `trigger` × `read`. An extension that
  refused every Operation whatever its verb now also hides Triggers.

## 2.19.0 · Probed opening of WebSockets, Run Summary

`pdo-extension-api` goes to **0.10.0**: addition only, an extension 0.1 to 0.9 compiles and
runs as is (`OLDEST_COMPATIBLE_API_VERSION` stays `0.1.0`). Detail: the decision records
and `docs/reference/extension-ui.md` ("WebSockets").

- **Probed opening**: an interface WebSocket opens, or reopens, only after a
  successful HTTP probe (`GET /actors/me`); as long as it fails, the probe repeats (backoff with
  jitter, ~1 s to ~30 s), a hidden tab does not probe. This covers the event stream, the terminals and,
  through `openProbedSocket` of the `@pdo/extension-ui` module (0.10.0), the extension interfaces.
  A tab opens only one event stream, the Review page included. The terminal of a node
  reattaches by itself after a daemon restart (`reconnect: "after-drop"`); closed by the
  daemon (session ended), it stays closed.
- **Clean closes**: `/ws` and the PTY answer the Close frame of the client.
- **Log**: one line per opening, closing and refused upgrade of a WebSocket.
- **Cache-Control**: `assets/` (hashed names) `immutable`, the rest `no-cache`.
- **Run Summary**: new table `run_summaries`, rebuilt from the event log at the first
  start (and after a daemon that did not maintain it); events written in the database of a
  running instance, outside the daemon, are caught up on the next read. `GET /runs`, `GET /runs/{id}/children`
  and the access checks of a Run read it; HTTP contract unchanged. The front groups its
  refreshes (~300 ms) and no longer polls the children of a Run or the I/O of a node as long
  as the stream is open.
- The `performance` marks `ws:ouvert` (opening of the stream) and `terminal:pret` (first byte
  of a PTY) are set by the interface.

## 2.18.0 · Run creation seams and UI locations `stats/tab` and `settings/page`

`pdo-extension-api` goes to **0.9.0**: additions only, an extension 0.1 to 0.8 compiles and
runs as is (`OLDEST_COMPATIBLE_API_VERSION` stays `0.1.0`). Detail: the decision record
(amendments), `docs/reference/extension-api.md` ("The creation of a Run") and
`docs/reference/extension-ui.md`.

- **Extension data of a Run**: `POST /runs` takes `extensions` (extension id -> free
  JSON value, also in multipart). An id that is not installed answers `400` like an unknown field;
  the extension concerned validates its value (`Extension::validate_create`) after authorization and
  before any effect: a refusal answers `400` with its reason, with no Run and no event. The value is
  frozen in `RunStarted` (`extensions`) and a `retry_all` carries it over to the twin.
- **Marking before the first Node**: `Extension::on_create_with_data` receives the value
  (default: `on_create`). The attributes of a Run are now set **before** the launch of
  its first Node, for a gesture as for a Trigger fire; the twin of a `retry_all` is
  marked too (it was not before).
- **`Host::create_run(actor, NewRun)`**: an extension creates a Run on behalf of an Actor, with the
  same authorization and the same hooks as `POST /runs`.
- **CLI**: `pdo run create ... --ext <id>.<key>=<value>` (repeatable).
- **New Run**: a `run/new` section can provide a value (`SectionForm.provide`), sent
  under the id of its extension; the `run/new` context gains `input` and `name`.
- **`stats/tab`**: an extension adds a tab to the Stats rail, after Performance, rendered
  in the Stats frame; its context carries the displayed period and the Refresh counter.
- **`settings/page`**: an extension adds a whole page to Settings, one rail entry
  per contribution after "Extensions" (which still lists the `settings` Sections);
  reachable through `#/ext/<extension>/settings/page/<path>`. The Save footer of Settings is
  hidden on such a page: it saves in its own way.
- The shared module `@pdo/extension-ui` goes to `0.9.0` with the API (same major `0`).
- The test extension of the core refuses a value starting with `refuse`, marks `data.<key>`,
  traces the order marking / launch (`GET /ext/test/creations`), creates a Run through its Host
  (`POST /ext/test/runs`) and its `run/new` section provides `cle`; it also contributes the
  "Test stats" and "Exploding" tabs and the "Test settings" page.

## 2.17.1 · The core names no extension

No contract change: `pdo-extension-api` stays at **0.8.0**. Detail: the decision record.

- Doc comments (and regenerated TypeScript types), Rust and front tests,
  `docs/reference/extension-api.md`, `docs/reference/extension-ui.md` and the past entries of
  this file describe each seam for itself, with the test extensions of the core (`test`,
  `demo`, `a/panneau`, `gate`) and a sample distribution, `acme-pdo`. Same assertions,
  other data.
- A repository created from scratch signs its initial commit with its own identity (`user.name` /
  `user.email`), even when `GIT_AUTHOR_*` / `GIT_COMMITTER_*` variables are inherited.

## 2.17.0 · Identity and rights seams

`pdo-extension-api` goes to **0.8.0**: additions only, an extension 0.1 to 0.7 compiles and
runs as is (`OLDEST_COMPATIBLE_API_VERSION` stays `0.1.0`). Detail: the decision record
(amendments) and `docs/reference/extension-api.md`.

- **Projects of an Operation**: an `Operation` carries `projects` (empty: outside any Project).
  The core fills it in: a Run, its nodes, its sessions and its Review comments are in the
  Project that owns the primary repository of the Run; a Project in itself; a repository in its
  Project; the rest outside any Project. On the wire (`/ws`, `on_event`), an Operation gains
  `"projects": [...]`. An extension fills `projects` for its own Resources. A repository
  written another way (`/repo/`, `/repo/.`, a link to `/repo`) stays in the Project of `/repo`.
- **Resource catalogue**: the `Manifest` declares the Resources of an extension
  (`with_resource(ResourceDeclaration)`); `GET /resources` and `Host::catalogue()` return the
  19 Resources of the core (label, help, verbs) then those of the extensions.
- `Runs::authorize`, `Review::authorize` and the session use cases that authorize become
  `async` (they read the Project of a Run); these workspace-internal APIs are not in the
  contract.
- **Run creator**: `RunStarted` freezes `created_by` (the Actor of `run` × `create`, the Trigger
  for a fire) and, for a triggered Run, `trigger_editor` (the Last editor of the Trigger). Both
  are in `GET /runs/<id>`; `created_by` also in `GET /runs`. The Info tab of a Run
  displays "Created by" (a Trigger by its name). A Trigger stores `last_editor` (column added to `triggers`), updated
  on each creation and `PATCH`, displayed in its detail.
- **Node launch**: new hooks `Extension::node_env` (variables of an agent or a
  script, host and sandbox, resume included; a core variable is never overwritten) and
  `Extension::node_preamble` (text at the head of the preamble of an agent). `NodeLaunch` gives them the
  Run, the Node, the creator, the Trigger and its last editor, the Project.
- **`CLAUDE_CONFIG_DIR`** provided by an extension: frozen on the `NodeStarted`
  (`claude_config_dir`), the cost and the liveness of the NodeRun read its transcripts below it.
- **Suspended Trigger**: hook `Extension::trigger_suspension`. A suspended Trigger no longer fires
  (`skipped-suspended` in its history, `409` on "Run now", which names the Trigger and the reason); `GET /triggers` returns it with
  `suspension: {extension, reason}`, the UI shows it with the switch off, an amber
  "suspended" chip (which moves under the name when the column is narrow, and truncates on hover rather than
  going under the actions), the reason in a tooltip and
  in the detail.
- Extension context: `trigger/item` and `trigger/item/menu` receive `last_editor` and
  `suspension`; `run/info` receives `created_by` and `trigger_editor`.
- The test extension of the core (the demo daemon of the Feature Paths) learns to inject
  variables (`PDO_TEST_NODE_ENV`, `PUT /ext/test/launch`), a preamble
  (`PDO_TEST_NODE_PREAMBLE`) and to suspend a Trigger (`PUT /ext/test/suspensions`).
- **Authorized reads**: new verb `read` (`Verb::Read`). Listing and viewing a Run
  (every `GET /runs/{id}...` read), listing and viewing a Project, browsing the disk
  (`GET /fs/browse`, `repository` × `read`) go through the Operation authorization: when refused, an
  entity disappears from the lists, its detail answers `403`. `/ws` relays to a connection only the
  events of the Runs its Actor can read (creation included; a `trigger_fired` arrives there without
  the `run_id` of an unreadable Run). **Breaking for an extension** that refused every
  Operation without looking at the verb: it now also hides Runs and Projects.
- **Authorization question**: `POST /authorize {"operations": [...]}` returns one decision
  per Operation without executing anything; an `Operation` deserializes. The New Run form uses it
  (magnifier hidden, repositories grouped by permitted Project, "N Project(s) hidden", "New Run" disabled
  when no Project allows it, question asked again every 15 s and when the window regains focus
  as long as it is, prefill limited to the proposed repositories); the `run/new` context gains
  `targetProject`. `GET /repos/recent` now lists only the repositories of readable Runs. Measured:
  500 Runs listed in ~35 ms (optimized build) with an extension that authorizes.
- **Admission of a request** (`Extension::admit`): an extension can refuse (`401`
  `unauthenticated`) or redirect to its login page a request that no extension
  identified, on all routes, `/ws` and PTYs included, never on the interface files.
  The interface follows the redirect before rendering anything (`next=` brings back
  the deep link, or the open Run when the session is lost along the way). Without an extension
  that asks for it, nothing changes.
- **`PDO_TOKEN`**: every CLI command that talks to the daemon sends it as
  `Authorization: Bearer`. New command `pdo run list [--json]`. An admission refusal reads
  in the same words everywhere, `pdo run create` included.
- **Display name of an Actor** (`Extension::display_name`, `Host::display_name`,
  `GET /actors/me`, `POST /actors/display`, `useCurrentActor` / `useActorDisplay` of the shared
  module); the audit log (`GET /audit`) returns `actor` and `actor_display` (column `actor`
  added to `audit_log`, without backfill: old rows have no Actor); the row of a
  Run names its creator when an extension names it.
- New UI location `topbar/account` (Decoration, after Settings) for the avatar menu.
- New top-level routes `/actors/*`: a reverse proxy that filters by prefix must
  let them through.

## 2.16.0 · Deep link to a Run, core factory out of GitHub issues

No crate change: `pdo-extension-api` stays at **0.7.0**. No daemon change.

- `#/runs/<run_id>` is a core deep link: following it closes the open extension page,
  selects the Run in the list and opens its detail. It works from the address bar, a markdown
  link, a page that writes `location.hash`, and `CommandHost.open`, which accepts it in addition
  to `#/ext/...`. An unknown Run shows an error toast and changes nothing. Reference:
  `docs/reference/extension-ui.md`, "Toward a Run".
- The core factory leaves GitHub issues: its business and technical tracking moves into a
  tool of the instance (dev tooling, `docs/agents/`), no more `gh issue` or `Closes` references
  for new work. GitHub issues still open stay there until they are closed.

## 2.15.0 · Overview tour, on a real Run of script nodes

No API change: `pdo-extension-api` stays at **0.7.0**. No daemon change.

- New *Overview* tour, first in the welcome modal and in Settings > Tutorials;
  the full tour chains *Overview* -> *First run* -> *First pipeline*. It works without a harness.
- The intro card prepares the training repository, the `tutorial-overview` pipeline (layout
  of `implement-review`, `script` nodes), a sample Trigger whose guard always refuses,
  and a Run awaited until `completed`; replaying reuses the last finished, non-archived Run. *Start*
  stays inactive until then.
- A 17-step visit, including 7 gestures that only advance on the observed state: Run, implementer
  (outputs, frozen terminal), edge reviewer -> End, Start, real archiving (skippable), Triggers,
  Pipelines. Settings and Stats are highlighted without being opened. The canvas is read-only
  during the tour.
- The sample Trigger is deleted on any exit from the tour (end, abandon, chaining of the full
  tour); a cleanup failure is reported on the displayed card. The pipeline and the Run stay.
- The third Happy Path now starts with *Overview*.

## 2.14.0 · Pipeline info as the only metadata surface, right panel collapsed

No API change: `pdo-extension-api` stays at **0.7.0**.

- Pipeline info (`i` in the toolbar) becomes the only surface for the metadata of a pipeline.
  On a template, the Info tab shows Identity (Name, Version, Prompt required), Variables,
  Canvas (Global / S / M / L grid) and Stats; each change marks the tab unsaved, can be
  undone, appears in the YAML and is written on Save. On a Run, active or archived,
  the same values are read-only. The fake "Description" block disappears.
- Removed: the Pipeline Inspector. An empty selection on a template shows nothing on the right.
- The right panel collapses when it has nothing to show (template with no selection and
  Pipeline info closed, home with no tab): the canvas takes the full width and the handle is
  hidden. It reopens at once at the last chosen width as soon as some content asks for it; the
  collapse never overwrites the persisted width. No manual collapse. A Run with no selection keeps
  its panel, an archived Run its "Run archived" state.

## 2.13.0 · Provisioning block collapsed, explained and diagnosable

No API change: `pdo-extension-api` stays at **0.7.0**.

- The Provisioning block is collapsed by default where it is secondary (New Run, project
  edit, inspector of an isolated node, detail of a running node). It stays expanded in
  Settings > Sandbox & worktrees. Collapsed, its header summarizes the rules
  (`N inherited · M at this level`, or `none`) and a line explains what provisioning is.
- Each mode (Copy, Hardlink, Symlink) and the level bar carry a tooltip.
- A mode conflict or a preview error reopens the block by itself, and it no longer collapses as long
  as the problem remains. In New Run, the disabled Launch button explains why.
- Removed: the "Configure worktree provisioning..." button of project edit and the
  "Configure / Hide provisioning" toggle of the node inspector, replaced by the collapsible block.

## 2.12.0 · Merge node removed, convergence is carried by any node

No API change: `pdo-extension-api` stays at **0.7.0**. The Merge node is removed.

- The `merge` node type disappears from the editor, the model and the runtime. Convergence is
  carried by **any node**: connect several edges to an `agent`, it waits until each branch
  has fired or is dead, receives only the branches that fired and finds their code on the branch of the
  Run. The toolbar no longer has a Merge button.
- A YAML that still contains `type: merge` loads: the node becomes an isolated `agent`, with the
  ordinary "unknown node type" warning, and keeps its `branches` / `merged` ports. No alias, no
  `pdo migrate` rule.
- Only behavior difference: a convergence node whose branches are all dead is
  now skipped (the old Merge was excluded from auto-skip).
- The automatic merge resolver, dead in production since an earlier decision, is removed (`merge-resolver`
  prompt, `merge_resolution_failed` / `merge_resolver_*` refusals, "fan-out without
  Merge" lint, Merge advice of the importer). The TypeScript contract loses `MergeResolverInfo` and the
  `merge_resolver` field of `RunState`. The `MergeResolver*` events of old Runs stay
  readable: these Runs reopen, are costed and keep their Infrastructure row in Stats.
- A merge-back conflict puts the node in Interrupted; its banner now says "Merge-back
  conflict" instead of "Session died".
- Canvas: an auto edge that leaves on the right toward a target located behind (typically the
  output without `side` of an old Merge, above its End) no longer crosses its source card.
  It leaves through a leg on the output side and goes around the card.

## 2.11.0 · Leaving a terminal writes nothing to it, node terminal enlarged in an overlay

No API change: `pdo-extension-api` stays at **0.7.0**. Closing a terminal (switching
node, closing the run Shell) no longer writes anything into the tmux session: the Claude Code input
in progress stays intact on one line and a detached shell survives.
"Expand terminal" opens the enlarged terminal: the same session, without remounting, in an overlay
16 px from the edges on a darkened background, with a name · iteration · status bar, a frame in the color of
the status and the compact completion gestures in `awaiting_user`. You leave it with Minimize, Esc
outside the terminal or a click on the background; the enlarged state is never persisted.

## 2.10.1 · Closable secondary panel, modals bounded to the window

No API change: `pdo-extension-api` stays at **0.7.0**. The secondary panel of the
full-window shell (Stats > Cost > Pricing details, Settings > Skills bank) opens
under the header, which stays visible, with a title, the Esc hint and a ✕. "Pricing details"
becomes a toggle. The Edit project, agent profile, loops destruction and
tab closing modals are bounded to 85% of the window height: title and actions stay
visible, only the body scrolls.

## 2.10.0 · One-click copy of the raw content of an output

No API change: `pdo-extension-api` stays at **0.7.0**. In the detail of a node, a
"Copy raw file" button appears on the port row of an output (hover or keyboard focus) and
in the header of the artifact modal. It copies the raw file of the displayed iteration
(frontmatter included, markdown not rendered, source for an html output), including on an
archived Run, and displays "Copied!" or "Copy failed" with the reason in a tooltip. No button on
image outputs (ported from core 1.114.0).

## 2.9.0 · Editor: edge by drag only, Assistant and Info exclusive

No API change: `pdo-extension-api` stays at **0.7.0**. Gesture change on the editing
canvas: an edge is now born only from a drag from the border of a
node, now 4.5 px thick. Clicking a border then another node no longer connects anything: the
click selects the node. On a template, the "agent" glyph and `i` of the toolbar are
exclusive and follow the displayed tab of the Pipeline info panel.

## 2.8.0 · README media configurable by a config

No API change: `pdo-extension-api` stays at **0.7.0**. The README media tooling
is set by a config (binary of the demo instance, Scenes, fixture,
seeding, selection, destination, Playwright resolution): another repo calls it from
a checkout of the core, `make readme-media CONFIG=<file>` then
`make readme-media-publish CONFIG=<file>` (reference: `scripts/readme-media/README.md`).
The core goes through its own config: its commands and its published media do not change.
`make readme-media-export` / `-import` stay specific to the core and refuse a third-party config.

## 2.7.0 · Location catalogue: tabs, sections, decorations and filters

`pdo-extension-api` goes to **0.7.0**, by addition: an extension written for 0.1 to 0.6 compiles
and runs unchanged (see the amended decision record; reference: `docs/reference/extension-ui.md`).

- **Tabs**: `node/detail`, `trigger/detail`, `skill/detail`.
- **Sections**: `project/edit` (at creation as at editing of a Project), `run/new`,
  `node/config`, `pipeline/config`, `run/info`. A Section of `project/edit` or of `run/new`
  receives `form` (`SectionForm`): it keeps a draft and applies it in `onSave`, called
  after the host Save with the saved entity (its id and its Attributes included);
  `onCancel` on cancel.
- **Decorations**: `run/item` (line 2 of the right column), `trigger/item` (line 3,
  under the pipeline name), `pipeline/item`, `canvas/node`, `port/item`, `review/file` (which also accepts the Actions of 2.6.0), `statusbar`.
  Without a contribution, nothing is rendered.
- **List filters**: `triggers/filter`, `pipelines/filter`, `projects/filter` (above the
  Run and Trigger lists, grouped by Project, behind a "Project" label). A
  filter receives `label`, the declared label of its contribution.
- **0.1 slots**: `ListFilter` of Triggers, pipelines and Projects, `DetailTab` of a node, a
  Trigger, a skill and a Project are now rendered (converted to these locations).
- The tab bar of the info panel of a Run scrolls instead of overflowing.

## 2.6.0 · Action locations, "⋯" row menus, core menus on the shared `DropdownMenu`

`pdo-extension-api` goes to **0.6.0**, by addition: an extension written for 0.1 to 0.5 compiles
and runs unchanged (see the amended decision record; reference: `docs/reference/extension-ui.md`).

- **Fourteen Action locations**: `runs/header`, `run/item/menu`, `runs/bulk`, `run/toolbar`,
  `canvas/node/menu`, `canvas/edge/menu`, `node/toolbar`, `trigger/item/menu`,
  `pipeline/item/menu`, `pipelines/header`, `tab/menu`, `review/toolbar`, `review/file`,
  `skill/menu`. Each passes on, in addition to the selected Run and its Project, what is at stake
  there (`entity`, `selection`, `pipeline`, `review`).
- **"⋯" menu** on Run, Trigger and Pipeline rows: the verbs of the row, one
  group per extension, then the destructive entry.
- **Context menus** of the canvas, the tab bar and the skills tree on the
  shared `DropdownMenu`, same entries and same behavior; a disabled entry there is
  `aria-disabled` (keyboard focusable) instead of a native `disabled`.

## 2.5.1 · Fix: the v2.5.0 tag does not compile

The merge of a change into `main` (v2.5.0) called `self.registered` instead of
`self.registry.registered` in `Extensions::skills` (`pdo-kernel`): the crate did not
compile, so neither did any consumer of the tag. No API change:
`pdo-extension-api` stays at **0.5.0**. Pin `v2.5.1`, never `v2.5.0`.

## 2.5.0 · Agent surface: `pdo ext <id>` and extension skills

`pdo-extension-api` goes to **0.5.0**, by addition: an extension written for 0.1 to 0.4
compiles and runs unchanged if it does not use the new hooks
(see the amended decision record).

- **`pdo ext <id> <args...>`**: the CLI of an extension, relayed by the daemon (`POST
  /ext-cli/<id>`). Arguments and flags as is, standard input (pipe or file only),
  working directory, and identity of the node session: the extension sees `Actor::Agent{run, node}`
  from a session, the local Operator otherwise. Stdout and stderr come back byte for byte.
  Codes: those of the extension; `1` daemon unreachable or extension failing; `127` no
  extension `<id>`, or no CLI. `pdo ext` alone lists the extensions that have one.
- **Hooks `cli()` / `run_cli()`** (`CliSpec`, `CliInvocation`, `CliOutput`, `EXIT_NO_CLI`) and
  **`skills()`** (`ExtensionSkill`, `SkillFile`), neutral by default. `GET /ext` carries a `cli`
  field (`{summary}`) for each extension that declares one.
- **Extension skills**: seeded at each start in the Skills bank, in a folder
  named after the extension, under the id `ext-<extension>-<name>`, locked and marked
  `extension: {id, version}` (badge "Extension `<id>`" and version in the detail). Rewritten
  when the version of the extension or their content changes, removed when it no longer provides them,
  delivered to worktrees like any selected skill. Two additive columns on
  `skills` (`extension`, `extension_version`): an existing database opens as is.
- `pdo complete`, `pdo ext` and the whole agent CLI still do not build the extensions.

## 2.4.0 · UI locations hosted by an extension

`pdo-extension-api` goes to **0.4.0**, by addition: an extension written for 0.1, 0.2 or 0.3
compiles and runs unchanged (see the amended decision record; reference: `docs/reference/extension-ui.md`
§ *Hosted locations*).

- **Hosted locations** (`HostedLocation`, `UiContributions::host`): an extension declares
  its own locations, `<id>/<name>`, with the accepted contribution type (`UiElementKind`)
  and the shape of their context. Others contribute to them through `UiLocation::Hosted("<id>/<name>")`.
  A contribution to the location of an absent extension is ignored.
- **Shared module `@pdo/extension-ui`**, in the import map of the front next to React:
  `useHostedContributions`, `HostedContribution`, `HostedContributions`, `runHostedCommand`. A
  bundle that imports it declares so (`UiContributions::importing_ui_module`); another major
  is not loaded, with a banner.
- **Wire of `GET /ext`**: `ui.locations` and `ui.ui_module_major` are added.
## 2.3.0 · Interoperability: extension resource and Host

`pdo-extension-api` moves to **0.3.0**. An extension routes its own mutations through the core's
mechanisms, so that two extensions installed together interoperate without naming each other.

- **`ResourceKind::Extension { extension, kind }`** (`ResourceKind::extension("test", "note")`),
  named `test/note` on the wire. Core resources keep their names (`agent_profile`...).
- **`Host`**, in `ExtensionContext::host` at `start`: `authorize` (all extensions, the emitting
  one included; a panic denies), `publish(ExtensionEvent)` (on `/ws` and to the other
  extensions, envelope `{"type": "extension_event", "extension", "name", "payload"}`),
  `applied(OperationApplied)` after a successful mutation, `on_create` (the Attributes of the other
  extensions, prefixed, to be stored by the extension). `NeutralHost` for an extension's tests.
- **`Denied`** and `OPERATION_REFUSED` move into the API (`Authorization::into_result`): an
  extension answers its denials with the core's `403`.
- **`OperationApplied` is broadcast on `/ws`** as soon as an extension is loaded. Without an
  extension, `/ws` does not change.

### Breaking for an extension (0.2 -> 0.3)

- `ResourceKind` is no longer `Copy`: clone where a copy used to be enough (`.clone()`, `.cloned()`).
  Its serialization is now written by hand; the names of the core resources do not change.
- `ExtensionContext` carries a `host` field and no longer implements its traits through `derive`
  (same behavior, equality included). `ExtensionContext::new(data_dir)` remains valid.
- An extension that did not rely on `Copy` compiles unchanged and
  is still accepted: `OLDEST_COMPATIBLE_API_VERSION` stays at `0.1.0`, since the breakage only
  affects compilation.

## 2.2.0 · UI location catalog, Commands, `when`, `topbar/end`, deep links

`pdo-extension-api` moves to **0.2.0**, by addition: an extension written for 0.1 compiles and
runs unchanged (reference: `docs/reference/extension-ui.md`).

- **Catalog of named locations** (`UiLocation`): `topbar/start`, `topbar/end`, `settings`,
  `runs/filter`, `run/detail`. An extension declares `UiContribution`s there (Page, Action,
  Tab, Section, Decoration, list filter) through `UiContributions::contribute`. The four
  0.1 slots (`UiSlot`) are still accepted and are converted to their location.
- **Commands** (`UiCommand`, `UiContributions::command`) and **`when` clauses** (`When`).
- **Context**: every component receives `context`, with the selected Run and its Project.
- **`topbar/end`**: icons before Stats, full-screen Page without a rail; `FullWindowShell` has an
  optional rail.
- **Deep links** `#/ext/<id>/<location>/<path>`, clickable in all markdown renderings of the
  core.
- **API compatibility**: the API declares `OLDEST_COMPATIBLE_API_VERSION`; the daemon accepts an
  extension that requires a version between this one (0.1.0) and its own, instead of only the same
  minor in 0.x.
- **Wire format of `GET /ext`**: `ui.slots` becomes `ui.contributions` (the 0.1 slots already
  converted) and `ui.commands`. Only the front end of the same binary read it.

## 2.1.0 · Third-party distributions: `main_with`, prebuilt frontend, update channel, 404 under `/ext`

Seams for third-party distributions. Backward-compatible addition
to the contract of `crates/pdo`: the CLI, its exit codes and the daemon do not
change (`--help` output identical for every subcommand). `pdo-extension-api` stays
at 0.1.1.

### `pdo::main_with(extensions)`

- **`pdo::main_with(extensions)`**, covered by semver alongside `serve`, `Config` and
  `Daemon`: command-line parsing, dispatch, exit code. The vanilla binary is now just
  `pdo::main_with(Vec::new)`. A distribution writes the same `main` with its extension factory
  and depends only on `pdo-daemon` and `pdo-extension-api`: no more copied dispatch, no more
  crates outside semver to pin.
- **`pdo::main_with_config(extensions, configure)`**: same, with `configure` applied to the
  `Config` of `pdo daemon` before `serve`. This is how a distribution declares its
  `ReleaseChannel`.
- The factory (`impl FnOnce() -> Vec<Box<dyn Extension>>`) is called only by `pdo daemon` and
  `pdo service`, once. The agent CLI (`complete`, `fail`, `wait-user`, `run create`...) never
  builds the extensions.
- **`pdo service` goes through the binary's extensions**: `install` and
  `uninstall` are subject to their Operation authorization. Without an extension, nothing changes.
- The exit codes of `pdo wait-user` (`3`) and `pdo run wait` (`2`) now have their tests
  at the binary level, like those of `pdo complete`.

### Embedded frontend without Node on the distribution side

- `crates/pdo/build.rs` builds the frontend into `$OUT_DIR/frontend-dist`, never again into
  `frontend/dist` nor into the cargo checkout. **`PDO_FRONTEND_DIST=<absolute path>`** embeds an
  already built `dist/`, without Node or pnpm: this is the path for a git tag dependency.
- Without `frontend/node_modules` or `PDO_FRONTEND_DIST`, the build fails with a message that names
  both remedies, instead of relying on pnpm's auto-install.
- **Breaking for core development**: `PDO_SKIP_FRONTEND_BUILD` (non-empty value) no longer embeds
  `frontend/dist`, but what `OUT_DIR` already contains. To embed `frontend/dist`, use
  `PDO_FRONTEND_DIST=$PWD/frontend/dist`. A release build refuses an empty frontend.

### `/ext/<id>` of an absent extension answers 404

- The whole `/ext/` prefix answers a JSON `404` when the extension is not loaded or does not serve
  a route (`{"error":"extension_not_found","message":...,"id":...}`), whatever the method,
  instead of the SPA's HTML with `200`. A path that a loaded extension does not serve answers
  `{"error":"not_found",...}` (the old body `{"error":"not found"}` is gone).

### Configurable update channel

- **`Config::with_release_channel(ReleaseChannel { name, installed_version, source_url,
  upgrade_command })`**, covered by semver: a distribution declares its source, its version and
  its upgrade command. Without a command, Update is refused (`409`); without a source, no
  check is made.
- Vanilla v2 no longer pointed to itself: the source and the installer targeted the v1 repository,
  and Update ran the v1 formula. The Homebrew command becomes `brew update && brew upgrade
  Loulen/pdo-v2/pdo-v2`, and **no check is made by default** as long as `PDO-v2` does not
  publish releases (`PDO_UPDATE_SOURCE_URL` remains the override).
- `GET /update` adds `channel_name` and `core_version`; `source`, `source_url` and
  `manual_command` can be `null`. The Update button is offered only if a newer version is known
  and the daemon would accept it.

## 2.0.0 · PDO v2: extensible architecture

First number of the **PDO v2** line, which lives in the private repository `Loulen/PDO-v2` and
is installed with the `pdo-v2` formula (tap `Loulen/homebrew-pdo-v2`), next to PDO v1 which stays
on the 1.x line of the public repository. No code change compared to 1.112.1: this major bump
marks the split of the two lines and the whole story, whose entries 1.109.1 to
1.112.1 below give the detail.

- The daemon is a hexagonal modular monolith: twelve context crates, `pdo-kernel`,
  `pdo-extension-api` (extension API 0.1, its own semver) and the composition root `crates/pdo`.
- The story's issues were recreated in `Loulen/PDO-v2` under new numbers. Commit messages and the
  entries below keep the old numbers.
- Only breaking point for the operator, already noted in 1.112.1: `RUST_LOG` targets in
  `pdo_daemon...` no longer match anything (see `docs/reference/runbook.md`).

## 1.112.1 · Contract: legacy crate, snapshot suite and ratchet removed

Refactor with no change visible to the operator: the HTTP/WS contract does not change. The
temporary snapshot suite was run one last time, green, on the final tree before being
removed. Only change to note: **`RUST_LOG` targets in `pdo_daemon...` no longer match
anything.** The old target -> new target table is in
[`docs/reference/runbook.md`](docs/reference/runbook.md). The default filter goes from
`pdo_daemon=info,info` to `info`, which comes to the same thing.

- The legacy crate (`crates/pdo-daemon`, package `pdo-legacy`) no longer exists. Only the
  twelve context crates, `pdo-kernel`, `pdo-extension-api` and the composition root
  `crates/pdo` remain. The remainder was distributed as follows:
  - into the composition root: daemon startup (`src/daemon/`: base, fix-ups, background loops,
    handle and its test seams), the wiring of each context (`src/contexts/`, one
    file per context), the CLI (`src/cli/`), the embedded SPA (`src/spa.rs`, and the `build.rs`
    that builds the front end), and the ports that read several contexts (`src/ports/`: the
    Run facts of the session context, the mounts that an archiving releases, the Runs seen by the
    platform, the Trigger fires counted by Stats). The cyclic global state (`AppState`)
    is gone: a context wired after the port that reads it is reached through a `Late` handle;
  - into `pdo-run`: the Runs that use a pipeline (`adapters/pipeline_runs.rs`);
  - into `pdo-settings`: who names a skill, who names a staging profile, and the real type of
    what a Project carries (`adapters/contexts/`).
- The `crates/pdo/tests/snapshots_temporary/` suite (8 scenarios and the test of its masks),
  `scripts/layout-ratchet.sh` and its CI step are removed, as is the `insta` dependency.
- Tests: no tier 4 test lost. The internal tests of the legacy crate move into the
  composition root, against the contexts wired the way the daemon wires them on an in-memory
  database (`contexts::testing`): declared wait, pane, `/sessions` gauges, reaper doctrine,
  historical `stale` Run (`ports/session/`), CLI (`cli/`), nested posture and relaunch command
  (`daemon/env.rs`), announced URL (`daemon/origins.rs`), SPA (`spa.rs`), Trigger fires
  (`ports/trigger_fires.rs`); the type of the Project settings moves into `pdo-settings`. The two
  service health tests no longer force the supervisor (the probe reads it from the
  environment): they check that the relayed value is the platform's. The architecture test
  now checks that it covers exactly the twelve contexts.
- The baseline of the 400-line test is empty. These were split: `App.tsx` (state hooks and
  panels of `app/`), `NodeInspector` (sections under `pipeline/components/nodeInspector/`),
  `EdgeDetailPanel`, `OrthogonalEdge`, `editNodeDerivation`, `anchorSide`, `serializePipeline`,
  and the tours (`lib/tour.ts`, `lib/tours/*`, `TourCards`, `useTour`); the `lib.rs` of the
  legacy crate was dissolved.
- Front end: the old flat folder `frontend/src/components/` is gone. The pipeline editor
  joins `pipeline/components/`, the tours `app/tour/`, the left panel `app/`, the generic
  components `shared/ui/` (`FullWindowShell`, `SelectControl`, `ConfirmDeleteModal`,
  `ImageLightbox`, `MermaidDiagram`, `ThemeSelect`), `ServiceHealthIndicator` `platform/` and
  `AgentControl` `settings/`.
- Docs: `docs/agents/module-layout.md` describes the final tree (the "during the migration"
  section is removed), `CONTRIBUTING.md` says where the code lives, the runbook is new
  (`docs/reference/runbook.md`), and the Vite proxy says where the router lives.
- Playwright specs: 45 of them were already failing on the integration branch (same
  list before and after this ticket). They pass again, with no product change except one
  fix:
  - the harness: the "Library" tab of the left panel has been called "Pipelines" for a while,
    and the welcome modal of the guided tour, which any database without a Run displays, is marked
    as already offered (`storageState` of `playwright.config.ts`);
  - specs that followed an old UI: the header of a branch group carries its
    counter, the attachments field no longer has an `accept` attribute, the Pipeline Manager
    starts on demand, the YAML tab shows the saved document, the control line of the
    review is chosen in the README as it is;
  - a fix: an automatic edge between two cards in a column left through the
    right side and went back through its source card; drawn above the nodes, it
    captured the click on the node name. It now goes around its source card through the shortest
    corridor (automatic routes only, nothing is persisted).

## 1.112.0 · Extension API 0.1.1: entity Attributes, events, `/ext/` routes and UI locations

Feature addition, backward-compatible: without an extension, the HTTP/WS contract does not change
(identical temporary snapshots, no UI element added). The only additions are those
opened by the extension API.

- **Entity attributes.** When a Run, a Project, an agent profile, a Trigger or a library
  pipeline is created, extensions set their attributes
  (`on_create`), prefixed by their id (`<id>/<key>`) and stored outside the event log (table
  `entity_attributes`). They are returned with the entity (`attributes`, absent when empty), and
  `GET /runs`, `/projects`, `/settings/agent-profiles`, `/triggers` and `/pipelines` accept
  `?attr.<id>/<key>=<value>`.
- **Events.** `on_event` receives the exact JSON of `/ws`, plus `operation_applied`
  (`{actor, operation, entity}`) after every authorized and successful mutation. Delivery is
  asynchronous and isolated: one thread and one bounded queue per extension, so that a
  slow or panicking extension never delays or blocks the core.
- **Routes and data.** An extension's routes are mounted under `/ext/<id>/` (prefix
  added to the Vite proxy); `GET /ext` lists the extensions and their UI contributions. Each
  extension receives its data folder `~/.pdo/extensions/<id>/` (new `start` hook).
- **UI locations** (`frontend/src/shared/extensions/`): navigation entry (top bar),
  settings panel ("Extensions" entry of Settings), Run list filter,
  Run detail tab. The front end imports the extension's ES bundle at runtime and shares
  its React with it through an import map; a bundle of another React major is not loaded and
  a banner reports it.
- `pdo-extension-api` moves to 0.1.1 (backward-compatible additions: `start`/`ExtensionContext`,
  `UiContributions` and its `UiSlot`s, `OperationApplied`).

## 1.111.15 · `settings` context: instance configuration hub dissolved, agent profiles

Refactor with no change visible to the operator: the HTTP/WS contract does not change (HTTP
and WS snapshots identical; `GET/PUT /settings` and `/settings/agent-profiles*` answer as
before, byte for byte, without an extension).

- The `instance_config` hub is dissolved: each context owns its instance defaults (environment
  variable, built-in value, `stored -> env -> default` resolver and disclosure on the
  page) in its own `adapters/instance_defaults.rs`: `pdo-run` (session cap,
  auto-completion, naming, `auto_fail`, manager, attachment budget), `pdo-session` (reaper and
  assistant TTL, default model and harness), `pdo-trigger` (guard timeout),
  `pdo-review`, `pdo-platform`, `pdo-sandbox` (default sandbox, staging profiles, Docker).
  The kernel gains the common shape of this disclosure (`instance_default::Disclosed`).
- The context leaves `pdo-legacy` for `crates/pdo-settings`: it keeps the stored tier (the
  singleton row) and assembles the page from the contexts' sections. It depends on the other
  contexts, never the reverse; it implements the ports through which the upstream contexts read
  their settings (`InstanceSettings`, `ReviewSettings`, `TriggerSettings`,
  `UpdateCheckSetting`), wired by the composition root.
- Each mutation is a use case that receives the Actor and first consults the Operation
  authorization: editing the settings (`instance_settings` x `update`), creating, editing
  and deleting an agent profile (`agent_profile` x `create` / `update` / `delete`).
  The startup warning about removed sandbox settings and the preheating of the harness
  catalogs are daemon transitions and ask for nothing.
- Tests: use cases against in-memory ports (tier 2) replace the `oneshot` tests of the
  legacy crate (`/settings`); the store tests (`instance_config`, `agent_profile`) and the
  page tests through the context's router move to tier 3 (`crates/pdo-settings/tests/adapters/`).
  The coverage of extension denials extends to the four mutation routes of the context. The
  legacy crate's router no longer declares any route.
- The `RUST_LOG` targets change with the code: `pdo_daemon` (settings handlers,
  startup warning) becomes `pdo_settings::application` (`::boot`,
  `::agent_profiles::read`...).
- Front end: the Settings surface (split by section under 400 lines), the agent profiles panel and modal,
  their stores, models and API move into `frontend/src/settings/`.

## 1.111.14 · `trigger` context: cron, guard, fires

Refactor with no change visible to the operator: the HTTP/WS contract does not change (HTTP
and WS snapshots identical; `/triggers*` answers as before, byte for byte, without an extension).

- Triggers leave `pdo-legacy` for `pdo-trigger`: the Trigger and its history, the cron,
  the next fire, the fire decision and the plan of a tick, the guard bounds in
  `domain/`; one use case per gesture in `application/`; SQLite, `sh`, the heartbeat, the routes
  and what the context reads from the others in `adapters/`.
- Each mutation first consults the Operation authorization: creation, editing, deletion
  (`trigger` x `create` / `update` / `delete`), global pause (`trigger` x `pause`), guard
  dry run (`trigger` x `test_guard`). A fire is an Operation (`trigger` x `fire`): "Run now"
  requests it with the request's Actor (denial: `403 operation_refused` before any effect), a
  tick with the `Trigger` Actor (denial: no Run, the slot is consumed, an `error` row in the
  history carries the reason). Deciding that a Trigger is due asks for nothing.
- `GET /stale/health` is the Runs liveness sweep and stays in `pdo-run`.
- Tests: use cases against in-memory ports (tier 2) replace the `oneshot` tests of the
  legacy crate (`/triggers*` routes: creation and audit, repoint, `target_repo` refused, pause,
  deletion, guard dry run); the store tests (`trigger_store`) and the pre-cut fetch
  move to tier 3 (`crates/pdo-trigger/tests/adapters/`); the guard timeout precedence
  becomes a pure test, without environment mutation. The coverage of extension denials
  extends to the six Trigger mutation routes, and a tier 4 test proves that a cron fire
  asks for authorization as the `Trigger` Actor.
- The `RUST_LOG` targets change with the code: `pdo_daemon` (Trigger handlers and tick)
  becomes `pdo_trigger::application` (`::fire`, `::fire::tick`...), `pdo_daemon::guard_runner`
  becomes `pdo_trigger::adapters::guard_shell`.
- Front end: the Triggers tab, the detail panel, the guard verdict and output, the cron presets
  and their API move into `frontend/src/trigger/`, split under 400 lines per file.

## 1.111.13 · `stats` context: Overview, Cost, Performance, absorptions and price sync

Refactor with no change visible to the operator: the HTTP/WS contract does not change (HTTP
and WS snapshots identical; `/stats/overview`, `/stats/cost`, `/stats/performance`,
`/stats/absorptions`, `/stats/absorptions/member` and `/settings/cost-prices/sync` answer
as before, byte for byte, without an extension).

- The context leaves `pdo-legacy` for `crates/pdo-stats`: the three folds and their
  accumulators, the R-7 median and Tukey fences, the absorption rules (keys, resolution
  order, frozen scope, cascade of a removal) and the normalization of the price source
  are pure domain. The absorption rules of the decision records do not change.
- The aggregations read the event journal through the ports of the `run` context (`EventStore`
  and the new `EventWindows`, whose SQLite adapter carries the indexed queries), never
  the table directly. The absorptions store no longer reads the `events` table: a Pipeline's
  history is supplied to it by these ports before the write, which remains a transaction.
- Each mutation is a use case that receives the Actor and first consults the Operation
  authorization: Combine (`stats_absorption` x `create`), Uncombine (`stats_absorption` x
  `delete` on the member), price sync (`price_table` x `sync`). The extension
  API gains these two resources (`ResourceKind::StatsAbsorption`,
  `ResourceKind::PriceTable`, minor addition). The price refresh at startup and
  the absorption born from a rename are daemon transitions and ask for nothing.
- The Performance memo and the sync lock belong to the context's instance,
  no longer to a global state.
- Tests: use cases against in-memory ports (tier 2), the context on its real
  adapters (SQLite, files, transcripts) driven by its router (tier 3, the internal
  tests of the legacy crate moved), the coverage of extension denials extended to the three
  mutation routes.
- The `RUST_LOG` targets change with the code: `pdo_daemon` (price sync and
  refresh) becomes `pdo_stats::application::prices`, and the rename
  `pdo_stats::application::absorptions::record_rename`.
- Front end: `StatsCharts` (3.8k lines) is split by tab (Overview, Cost, Performance)
  in `frontend/src/stats/components/charts/`, with `StatsModal`, `StatsAbsorption`,
  `StatsAbsorptionsPanel`, their hooks, their models and their API (`frontend/src/stats/`).

## 1.111.12 · `review` context: diff, structured diff, review comments

Refactor with no change visible to the operator: the HTTP/WS contract does not change (HTTP
and WS snapshots identical; `GET /runs/{id}/diff`, `.../diff/structured` and `.../review/comments*`
answer as before, byte for byte, without an extension).

- A Run's review leaves `pdo-legacy` for its own crate, `crates/pdo-review`: the diff as data
  and its parser, the comment rules (ids, excerpt, author of a reply, filter, anchor re-map,
  effect of a reply or a decision, single message to the manager)
  in `domain/`; one use case per gesture and per read in `application/`, behind three
  ports (`ReviewRuns` on the `run` context, `ReviewGit` on the git adapter of the `workspace`
  context, `ReviewSettings` for `review_agent_can_resolve`, still implemented on the legacy side);
  the routes, the CLI (`pdo review list`, `pdo review reply`) and the adapters in
  `adapters/`. The composition root wires the context and merges its `router()`.
- Each gesture first consults the Operation authorization, even before reading the Run: sending
  (`review_comment` x `create`), reply, resolution, reopening (`review_comment` x `reply`,
  `resolve`, `reopen` on `<run>/<comment>`). The author of a reply comes from the Actor set
  by `identify` (the Agent of the manager's or a node's session). A denial answers
  `403 operation_refused` without writing anything.
- Front end: the review lives in `frontend/src/review/` (`api/`, `model/`, `store/`,
  `components/`): the Review page split into stores (Run and pair, diff and re-map, live,
  layout, state, comment list and gestures) and components (top bar,
  banners, body), the Diff tab, the cards and the file header, all under 400 lines.
- Tests: use cases against in-memory ports (`crates/pdo-review/tests/use_cases/`,
  tier 2), the context on its real adapters (git in a temporary repository, the real journal
  of the `run` context, `tests/lifecycle/`, tier 3), the coverage of extension denials extended
  to the four review mutation routes. Moves and removals:
  - the 16 internal tests of the legacy crate (`run_diff_*`, `structured_diff_*`,
    `run_refs_lists_deliveries_...`, `default_review_diff_...`, `send_review_comments_*`,
    `review_reply_*`) are moved as is into `crates/pdo-review/tests/lifecycle/`
    (`diff.rs`, `comments.rs`);
  - the unit tests of `structured_diff.rs` (parser) and `review_comments.rs`
    (ids, excerpt, message, author, filter, re-map) are moved next to their code in
    `domain/`; those that touched git (`compute_over_a_real_repo`, `compute_worktree_...`,
    `snapshot_worktree_...`, `ref_and_path_safety`) into `adapters/git.rs`;
  - `review_agent_can_resolve_stored_beats_env_and_defaults_off` is converted into a pure test of
    the `stored -> env -> default` resolution (`domain/comments/decision.rs`): it no longer depends
    on the process environment;
  - no test is removed.
- The `RUST_LOG` targets change with the code: the review handlers of `pdo_daemon`
  become `pdo_review::application::{run_diff, structured_diff, list_comments,
  send_comments, reply, decide, comment_events}`, the glue to the manager
  `pdo_review::adapters::runs`.

## 1.111.11 · `run` context, 3/3: Run surfaces and front-end Run views

Refactor with no change visible to the operator: the HTTP/WS contract does not change (HTTP
and WS snapshots identical; `POST/GET /runs`, `DELETE /runs/{id}`, `reapable`, `children`,
`artifact`, `file`, `refs`, `source-drift`, the run-scope pipeline, `PATCH repos` and the Pipeline
Manager answer as before, byte for byte, without an extension).

- The Run surfaces leave `pdo-legacy` for `pdo-run`: `lib.rs` no longer contains any Run
  handler (a Run's review, its diffs and its comments, waits for the `review` context). Creation
  is split by step (repositories, pipeline, frozen provisioning and isolation, `RunStarted`
  payload, the Run on disk, its start).
- Each mutation is a use case that receives the Actor and first consults the Operation
  authorization: creation (`run` x `create`), forgetting (`run` x `delete`), repositories
  (`run` x `edit_repos`), run-scope pipeline (`run` x `edit_pipeline`), Pipeline Manager
  (`run` x `start_manager` / `stop_manager`). A denial answers `403 operation_refused` before
  any write. Archiving and `retry_all` remain the consequence of an already authorized command;
  the `RunSurfaces` port is gone.
- The cost of a Run (`run_cost`), the price table (its tiers and their reading) and the Run
  refs join the context; price sync stays with the instance settings. The git reads shared by
  refs and review move into the `workspace` context (`adapters/git/revisions.rs`).
- `pdo run create` and `pdo reap` join the context's CLI.
- Tests: the surfaces' use cases against in-memory ports (tier 2), the internal tests
  of the legacy crate that covered this code moved into `crates/pdo-run/tests/lifecycle/surfaces/`
  (tier 3), the coverage of extension denials extended to the six new mutation routes.
- The `RUST_LOG` targets change with the code (`pdo_daemon` for creation, archiving,
  forgetting, repositories, the run-scope pipeline and the manager becomes
  `pdo_run::application::{create_run, archive, forget_run, edit_repos, run_pipeline, manager}`,
  `pdo_daemon::price_table` becomes `pdo_run::domain::prices`).
- Front end: the Run list, the launch modal, the node detail, the artifacts, the child
  Runs, a Run's repositories and their API move into `frontend/src/run/`, split under 400
  lines per file.

## 1.111.10 · `run` context, 2/3: node scheduler and lifecycle, agent commands

Refactor with no change visible to the operator: the HTTP/WS contract does not change (HTTP
and WS snapshots identical; the node routes, `/runs/{id}/commands` and `/stale/health` answer
as before, byte for byte, without an extension).

- The scheduler and the node lifecycle leave `pdo-legacy` for `pdo-run`: the pure
  scheduler evaluators (`domain/scheduler/`, with the dispatcher, stuck Run detection,
  the resume choice and the verdict of a Merge), the interpreter and its drivers
  (`application/scheduler/`), the spawn, the primitives, the prompt, the inputs and outputs, the
  reap and the reattach of a node (`application/node/`), the resume at startup, the
  liveness sweep and the preparation of a Run's sandbox. The functions of more than 400 lines
  (`spawn_node`, the command dispatch, the start primitive, the preamble) are split
  by step.
- Each gesture is a use case that receives the Actor set by `identify` (`Agent{run, node}` for
  the agent CLI) and first consults the Operation authorization: `CompleteNode`,
  `ReleaseCompletion`, `FailNode`, `SkipNode`, `StartNode`, `StopNode`, `RetryNode` (Operation
  `node` x action, target `<run>/<node>`) and `RunCommand` (a node command targets its node,
  the others the Run, after the `410` denial of a forgotten Run and the validation). A denial answers
  `403 operation_refused` before any write. The scheduler transitions never build any:
  an extension that refuses everything blocks no Run.
- The context reads the others through narrow ports (tmux and the declared wait, the Projects, the
  skill bank, the staging profiles); what remains legacy (the instance settings and
  the agent profiles, archiving and `retry_all`) is implemented in `run_ports.rs`.
- The node routes, `/runs/{id}/commands` and `/stale/health` move into the context's
  `router()`; the agent CLI (`pdo complete`, `fail`, `skip`, `wait-user`, `run wait`) into
  `adapters/cli/`. The constants `DEFAULT_PORT` / `DEFAULT_DAEMON_URL` move into the kernel,
  `DEFAULT_PROFILE_ID` into the `harness` context.
- Tests: use cases against in-memory ports (tier 2), the context on its real
  adapters driven by its router (`tests/lifecycle/`, the internal tests of the legacy crate
  that covered this code, moved), unit tests next to their code.
- The `RUST_LOG` targets change with the code (`pdo_daemon::node_spawn` becomes
  `pdo_run::application::node::spawn`, etc.; the table is in `docs/agents/module-layout.md`).
- Front end: the node API (prompt, inputs and outputs, Start / Stop / Retry), completion
  (`markNodeDone`, `releaseNodeCompletion`), the Pipeline Manager commands and the
  `useNodeRun` store move into `frontend/src/run/`.

## 1.111.9 · `run` context, 1/3: domain and event store

Refactor with no visible change: the HTTP/WS contract does not change (HTTP and WS snapshots
identical; `/ws`, `GET /runs/{id}` and `GET /runs/{id}/events` answer as before).

- New crate `pdo-run` (domain / application / adapters), foundation of the context's next two
  tickets: a Run's events, its state and the projection (pure function, split by
  event family) leave `pdo-legacy` (`event_log`), with the Run rules
  (`graph_resolver`, `loop_region`, `edge_router`, `switch_router`, `condition`,
  `variable_resolver`, `transition_guard`, `admission`, `blackboard`, `input_resolution`,
  `retry_verdict`, `restart_verdict`, `reap_policy`, `auto_fail`, `completion_refusal`,
  `child_awaiting`) and the folded part of the review comments.
- The event log goes behind the `EventStore` port (SQLite adapter). A single use case,
  `AppendEvent`, lets an event in: transition guard, write, then publication
  on the kernel bus. It is a transition: it does not consult authorization.
- The kernel bus carries exactly the JSON of `/ws`; `/ws` reads only from it. The two internal
  channels (Run events, announcements) become one, sized for both.
- The HTTP projection of the verdicts (completion, retry, restart) moves into
  `adapters/http/verdicts/`, and the reading of `PDO_SESSION_CAP` into `adapters/env.rs`: the
  domain no longer reads HTTP or the environment.
- What a Run's detail reads from disk (LOC, cost, the Run's pipeline) is a port that the legacy
  crate implements as long as these contexts live there (`run_ports.rs`).
- The `RUST_LOG` targets change: `pdo_daemon` (append, `/ws`) becomes
  `pdo_run::application::append_event` and `pdo_run::adapters::http::ws`, `pdo_daemon::event_log`
  becomes `pdo_run::domain::projection`.
- Front end: a Run's API (`fetchRun`, `fetchRunEvents`), the `/ws` stream (`useDaemonSocket`, its
  frames), the Run selection, the live node choice and the session counter move into
  `frontend/src/run/`.
## 1.111.8 · `pipeline` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots; the
`/pipelines*`, `/library*` and `/nodes/parse` routes answer as before).

- New `pdo-pipeline` crate (domain / application / adapters): the pipeline model and its parsing,
  the references it makes to itself, loop regions and cycles, the migrator, the portable
  document, the import of a Claude Code workflow, the node library, the identity of a pipeline
  (name ⇄ file) and the rules for an edit during a Run leave `pdo-legacy` (`pipeline`,
  `pipeline_migrator`, `portable_pipeline`, `workflow_importer`, `library_store`,
  `mutation_validator`, `frontmatter_parser`, `pipeline_watcher`). The instance registry, the
  library and the watcher are adapters.
- Every mutation (create, save, rename, duplicate, delete, import a document or a workflow, pin
  or remove a node from the library) goes through a use case that builds an Operation
  (`pipeline` or `library_entry`) and calls authorization.
- A pipeline's active Runs and the Stats absorption of a rename are ports that the legacy crate
  implements as long as those contexts live there (`pipeline_ports.rs`).
- `pipeline_semantics` and the `library_store::pipelines` module, which only served tests (the
  former pipeline library, removed earlier), are deleted along with their tests.
- The `RUST_LOG` targets change: `pdo_daemon::pipeline_watcher` becomes
  `pdo_pipeline::adapters::watcher`, `pdo_daemon::pipeline_migrator` becomes
  `pdo_pipeline::adapters::fs_migration`, `pdo_daemon` (pipeline handlers) becomes
  `pdo_pipeline::application`.
- Front end: the API, the editor store, the canvas, the info panel and the Pipelines tab move to
  `frontend/src/pipeline/`, split into files of at most 400 lines.

## 1.111.7 · Extension API: identity and Operation authorization

Without an extension, nothing changes: the HTTP/WS contract is identical (unchanged snapshots).

- `serve(config, extensions)` registers extensions after checking their manifest: a valid and
  unique id, and a compatible extension API version (`Manifest::api_version`, by default the one
  the extension is compiled against). Otherwise, the daemon refuses to start and says why.
- `identify` sets the Actor chosen by an extension; `authorize` is consulted by every mutation
  use case of the extracted contexts, never by the transitions the daemon performs on its own.
- A refused Operation answers `403` with `{"error": "operation_refused", "message", "reason"}`
  (the convention of the decision record). The `403` refusal responses of `platform`,
  `workspace`, `session`, `sandbox` and `skill` change from `{"error": "operation refused: …"}`
  to this body; the `skill` refusal loses its `"code": "denied"` field along the way (the
  `403` `locked` of the bundled skill keeps its own). The UI shows the reason in the gesture's
  existing error. A panic in `authorize` counts as a refusal and leaves a log line that names
  the Actor (for an identified Actor: `<namespace>:<id>`).
- Naming a group is **a single** `Create` Operation: `POST /projects` accepts an optional
  `members` field (the group's paths), attached at creation; a path that is already a member of
  another Project refuses the whole creation (`409` naming the owner, nothing is written).
  Without `members`, the request and the response are unchanged. The group pencil no longer
  sends an empty `PATCH` after creation, and if a later step fails, it keeps the created Project
  (a new attempt edits it, with no duplicate).

## 1.111.6 · `skill` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots; the
`/settings/skills*` and `/settings/skill-folders*` routes answer as before).

- New `pdo-skill` crate (domain / application / adapters): skill bank, folders, import from a
  Source, selection by tier, portable document sidecar and delivery into the worktree leave
  `pdo-legacy` (`skill_bank`, `skill_import`, `skill_selection`, `skill_sidecar`,
  `skill_delivery`). Every mutation goes through a use case that builds an Operation and calls
  authorization; the startup seed and the delivery (daemon and scheduler transitions) do not go
  through it.
- Delivery goes through a port (`SkillDelivery`); its adapter reads the index, `info/exclude`
  and the provisioning manifest through the git adapter of the `workspace` context, which also
  gains the shallow clone of a Source.
- A skill's referents (instance, Projects, Triggers, pipelines, Runs not started yet) are read
  through a port that the legacy crate implements as long as those contexts live there.
- Front end: bank, import, paste, Files tab, selector and tree move to `frontend/src/skill/`,
  split into files of at most 400 lines.
- Fix: `useDaemonSocket` no longer schedules a reconnection when the `close` event arrives after
  unmounting (second socket after a remount, timer firing after the end of a test).

## 1.111.5 · `sandbox` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots; the
staging profile routes and those of their referents answer as before).

- New `pdo-sandbox` crate (domain / application / adapters): staging profiles, image,
  container, home staging and Run preparation leave `pdo-legacy`. `run_sandbox.rs` keeps the Run
  side (a Run's sandbox context from its state) until the run context is extracted.
- Docker goes behind the `DockerCli` port (`ProcessDocker` adapter). Creating, editing and
  deleting a profile are use cases that build an Operation and go through authorization, with
  the Actor from the `identify` middleware and the same extension host as the other contexts.
  The session context ports (`SandboxExec`, `ContainerExec`) now rely on `pdo-sandbox`.
- Still to do: like workspace, the sandbox router is still mounted in `pdo-legacy`
  (`run_sandbox::router`) rather than through `contexts.rs`.
- Front end: the staging profile editor moves to `frontend/src/sandbox/`.

## 1.111.4 · `session` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots).

- New `pdo-session` crate (domain / application / adapters): tmux manager, PTY bridge, shared
  terminal, declared wait and library assistant leave `pdo-legacy`, along with the `/sessions*`,
  `…/nodes/{id}/pane` and `…/wait-user` routes.
- tmux goes behind a single port (`Tmux`): the legacy crate, the PTY bridge and the two shared
  terminal calls (`list-clients`, `refresh-client`) go through it. The manager (5.4k lines) is
  split by responsibility: session lifecycle, text sending, reaper, terminal driver.
- Every mutation of the context (wait, resume on Enter, shell, assistant, focus, save, take
  control, pop-out, pane resurrection) goes through a use case that asks for Operation
  authorization; the reaper and the wait set by the daemon on a refused completion remain
  internal transitions.
- `RUST_LOG` targets: `pdo_daemon::tmux_session_manager` becomes `pdo_session::adapters::tmux`
  (and `pdo_session::application::launch`), `pdo_daemon::pty_bridge` becomes
  `pdo_session::adapters::pty`.
- Front end: the terminal, the Run shell modal and the assistant panel move to
  `frontend/src/session/`.

## 1.111.3 · `workspace` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots; the
`/projects*`, `/repos*`, `/fs/browse` and `/settings/provisioning` routes answer as before).

- New `pdo-workspace` crate (domain / application / adapters): projects, provisioning,
  worktrees and repository creation leave `pdo-legacy` (`project_store`, `provisioning`,
  `worktree_ops`, `repo_scaffold`). Every mutation goes through a use case that builds an
  Operation and calls authorization.
- Git goes behind a port: a single adapter (`adapters/git`), tested against a real temporary
  repository, that the other contexts will call when they migrate.
- The legacy crate mounts the context's router and passes it the extension host through
  `DaemonConfig::extension_host`; going through `contexts.rs`, like `platform`, is still to do.
- Front end: project editing, `SourceBranchField`, provisioning editors, folder explorer and
  repository combobox move to `frontend/src/workspace/`.

## 1.111.2 · `platform` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots; the
`/pages*`, `/update*` and `/audit` routes answer as before).

- New `pdo-platform` crate (domain / application / adapters), the first extracted context and
  the template for the following ones: page mounting, update checking and applying, changelog,
  service unit and audit log leave `pdo-legacy`. Every mutation goes through a use case that
  builds an Operation and calls authorization.
- The `pdo page` and `pdo service` CLI moves to the context's `adapters/cli`. The legacy crate
  wires the extracted contexts through `contexts.rs`, the only seam the composition root uses.
- Front end: mounted pages, update, changelog and audit log move to `frontend/src/platform/`.

## 1.111.1 · `harness` context extracted

Refactor with no visible change: the HTTP/WS contract does not move (identical snapshots, the
`harness_descriptors` block of `GET /settings` identical to the previous version's).

- New `pdo-harness` crate (domain / application / adapters): registry, argv, per-harness
  capabilities, catalog, resolution and agent choice leave `pdo-legacy`.
- The probes ↔ cost / stale / staging / tmux / stats cycle is broken: the parsers for
  transcripts, reported cost and context peak live in the harness domain. On-disk transcript
  resolution and the binary probe become adapters; the catalog cache becomes a use case behind
  the `BinaryProbe` port.
- Front end: the harness, model and effort selectors move to `frontend/src/harness/`.

## 1.111.0 · Typed contract and hexagonal skeleton

Second ticket of the spec (three decision records). The HTTP/WS contract does not change
(identical snapshots); the only additions are the session headers sent by the agent CLI.

- Cargo workspace: `pdo-extension-api` 0.1 (`Extension` trait, neutral hooks, re-exports axum),
  `pdo-kernel` (clock, Actor, Operation, neutral authorization, event bus, extension host),
  `pdo` becomes the composition root (`serve(config, extensions)`). The legacy crate is now
  called `pdo-legacy`; its lib remains `pdo_daemon` (the `RUST_LOG` targets do not change).
- `identify` middleware at the head of the router: local Operator by default, Agent for
  `pdo complete` / `fail` / `skip` / `wait-user`.
- TS types generated by ts-rs in `frontend/src/shared/generated/` (`make types`);
  `frontend/src/types.ts` goes away and `make check` fails if the types drift.
- Discrepancies revealed by the generated types, fixed: a `waiting` Node (waiting for a session
  slot) is finally displayed ("Waiting") without looping polling, and the editor keeps
  `required` / `auto_fail` on save.
- Structure tests: an architecture test on a fixture, a ceiling of 400 lines per production file
  with a baseline that can only go down.

## 1.109.1 · Refactor safety net: covered routes and temporary snapshots

No production code touched. First ticket of the spec (three decision records): it lays the net
before the migration to a modular monolith.

- Integration tests for the 23 routes not covered until now (`crates/pdo-daemon/tests/routes_*`).
- `route_coverage`: every route of the router must be called by at least one test.
- Temporary snapshots (`tests/snapshots_temporary/`, insta) of the HTTP and WS journeys, with
  volatile values masked; they freeze the contract during the migration and will go away with
  a later ticket.

## 1.109.0 · Stats: absorption of Pipelines, Nodes, Models, efforts and pairs

The four tickets of the story, delivered together.

**Stats: absorb a model's efforts and a Node's pairs.**

- On the "By model" axis of Cost and Performance, select efforts of the same model
  (`not set` + `high`) then "Combine": one effort row, a `[⧉ N]` badge and a modal listing the
  members. A selection across two models is refused, in the UI and by the API (400).
- Global absorptions (Models, efforts) also merge each Node's model × effort pairs, in the
  Pipeline and Project trees. The affected pair carries a greyed "Global absorption" badge:
  read-only modal, link to the "By model" axis.
- In a Node's detail, any two pairs combine into one row. The absorption is local to the Node,
  never appears on "By model", and applies after the global one.
- Frozen scope: an absorption of efforts or pairs covers the raw keys present when it is set; a
  member absorbed later is not covered.
- Settings › Stats absorptions lists the "Effort · <model>" and "Pair · <Node>" entries.

**Stats: absorb Nodes and Models.**

- Under a single Pipeline, select two Node rows (Ctrl/Shift-click in the detail) then
  "Combine": a Node renamed from one version to another (`review` -> `code-review`) reads as a
  single row in Sessions, Cost (Pipeline, Project and Model trees) and Performance. The
  selection visibly refuses to mix Pipelines and Nodes, or Nodes of two Pipelines; the API also
  refuses the latter case.
- Removing an absorbed Pipeline takes its Nodes out of the Node absorption of its former
  absorber: they get their row back under their Pipeline.
- On the "By model" axis of Cost and Performance, two Model rows (a pinned alias and its
  observed id) combine; the absorber keeps its verbatim id, their efforts merge by effort, and
  the member list says whether each id was observed or requested. The Pipeline axis is not
  touched.
- Settings › Stats absorptions lists the Nodes (with their Pipeline) and the Models.
- Fix: the "Combine" bar and its modals were missing from the Performance tab.

**Stats: absorption on rename, absorptions list, Uncombined switch.**

- Renaming a Pipeline in the library (id changed) creates the old -> new absorption, with
  origin `rename`. A chain A -> B -> C gives C absorbing A and B; a display rename that keeps
  the id creates nothing.
- Settings › General › *Stats absorptions* (`saves as you go`) lists all absorptions, their
  absorber, their absorbed members and their origin; the cross removes an absorbed member at
  once.
- An *Uncombined* switch on Sessions, Cost, Performance and Triggers shows the raw figures
  (`uncombined=true`); it goes back to its default each time Stats is opened.

**Stats: absorb Pipelines.**

- In Stats › Sessions, select several Pipelines (Ctrl/Shift-click) then "Combine": a single
  row, named after the absorber, accumulates their Runs in Sessions, Cost, Performance and
  Triggers. The `[⧉ N]` badge opens the member list, from which an absorbed member can be
  removed.
- The absorption is stored in `pdo.db` and applied on read: the event log is never rewritten,
  and `uncombined=true` on the Stats routes returns the raw figures.


## 1.108.0
**Compact skill selector, "active skills" everywhere.**

- Collapsed, the selector fits on one line: icon, "Skills", sub-line "No skill" /
  "n active skills". The list of names under the button and the red warning paragraph are gone;
  a selected skill that no longer exists in the bank is flagged by an alert icon (message in a
  tooltip) and is not counted.
- Expanded: inherited skills checked, greyed out, origin tier on the right; missing skills
  listed struck through at the top, uncheckable; the popover stays open after each check and
  also opens in read-only mode, checkboxes frozen.
- The *First run* tour (step "Add the two PDO skills") reads the checked state in the popover.
- Vocabulary: "effective" becomes "active" on the skills side (front resolver types, frozen
  skill type on the daemon side, test identifiers, HP scenarios, documentation). Stored pipeline
  format unchanged.

## 1.107.1 · Showcase README: published media are those of the target pipelines

The README finally shows the scenes replayed on the target pipelines drawn by the maintainer.
Versions 1.104.0 to 1.107.0 had recorded them without publishing them: the hero, Visual
pipelines, Routing & loops, Triggers, Agent profiles, Skill bank and Stats GIFs still dated
from v1.

- **`docs/assets/readme/` republished**: the hero runs `implement-review` with its loop, the
  trigger launches `prod-check`, the displayed paths are `~/code/...`. The Routing & loops line
  describes `verdict = fail` / `verdict = pass`, like the target.
- **The hero passes the screen guard**: the `~/.claude/CLAUDE.md` of the demo instance forbids
  the live agent from typing an absolute path, which showed the throwaway root
  `/tmp/pdo-readme-media-*` in its terminal.
- **Agent profiles scene**: it opens the profile through its `Edit` pencil, in the modal of
  1.103.2.

## 1.97.4 · Showcase README, reference in `docs/reference/`

The README becomes a showcase (French translation: `docs/readme/README.fr.md`). The reference
it carried moves out with no loss: CLI commands, installation and update in
`docs/reference/cli.md`, reverse proxy and attachments in `docs/reference/reverse-proxy.md`,
copy-paste in `docs/reference/terminal.md`, development in `CONTRIBUTING.md`.

- **The harness support table lives in `docs/reference/harnesses.md`**, with the
  prerequisites. `make check` and `make support-table` target this file, and it is now the
  default `--file` of `pdo docs support-table` (before: `README.md`, which no longer carries a
  generated block).

## License · MIT, explicitly

The repository had no `LICENSE` file since its creation; only the Cargo metadata declared MIT.
The file now exists and confirms these terms; the contribution rules are in `CONTRIBUTING.md`.

## 1.103.2
**Agent profiles: only the Edit button opens the editor, in a modal shared with New
profile.**

- The pencil on each row of Settings › Agents › Agent profiles is a real `Edit <name>` button.
  Clicking the rest of the row no longer opens anything.
- The editor (Name, Harness, Model, Effort) leaves the form under the list for a modal. **New
  profile** opens the same modal, empty, with `Create`. The rules and REST calls do not change;
  Cancel, Esc or a click on the backdrop close it without saving anything.

## 1.103.1
**Saving a pipeline without changing its name no longer renames it.**

- A save only counts as a rename if `name:` really changes. Before, a file whose name contains
  an uppercase letter (`Mixed-Case.yaml`) was moved on every save, or refused with a 409 while
  a run was using it.
- A new pipeline takes the slug of its name as file name (`Bugfix-auto-clean` ->
  `bugfix-auto-clean.yaml`), like duplicate, import and rename. Its visible name does not
  change. An empty name is refused (400), and so is a name already taken (409).

## 1.103.0
**Wiring grid: 30px by default, adjustable S/M/L size.**

- The wiring grid step is no longer fixed at 40px: `S` = 20px, `M` = 30px (default),
  `L` = 40px. The decision record supersedes the previous one for the step.
- Global setting in Settings › General › Interface (per browser). A pipeline's Inspector panel
  can override it; this choice is saved in the pipeline file (`grid_size`) and travels with its
  `waypoints`.
- Changing the size never rewrites the waypoints already saved: old 40px routes keep their
  bends until they are touched again.

## 1.102.1
**The terminal no longer cuts off its last columns.**

- On a wide pane, the last 1 or 2 columns disappeared behind a black band on the right. The
  page's letter spacing, inherited by xterm, skewed the glyph width. The terminal container
  resets it to zero: tmux's last column is whole, whatever the pane width.
- The tmux status bar remains truncated by tmux itself (`status-right-length 40`,
  `status-left-length 10`): this is not a pane crop.

## 1.102.0
**Shared terminal: a single driver, read-only spectators, take control.**

- When two browsers open the same terminal (node or Manager), the first is the **driver**: only
  it weighs on the tmux window size. The following ones are **spectators** (tmux client
  `ignore-size`): they see the driver's whole screen, with a reduced font (6 px floor, scrolling
  beyond), and their keystrokes never reach the session.
- The driver sees an eye and the number of spectators; the spectator reads a read-only tooltip.
  When the driver leaves, the spectator is alone again: normal size, font and input.
- Two tabs on the **same** machine remain without a role, as before.
- A spectator sees a hand icon ("Take control") in the terminal bar. One click gives them
  control right away, with no confirmation: their window gets its normal size and font back,
  their keystrokes reach the session. The switch happens live (`refresh-client -f`), without
  reattaching a tmux client.
- The former driver becomes a spectator, briefly reads "Another browser took control" and keeps
  the hand icon to take control back. When the driver leaves, control goes to the machine that
  arrived earliest.
- During a declared wait, a spectator's banner says "take control to reply, then press Enter
  to resume".
- Known earlier issue: closing a terminal socket injects `\n^D` into the pane
  (`portable-pty`), to be handled in a separate ticket.

## 1.101.1
**The End node accepts an edge on all four of its sides.**

- End lands where the wire is dropped, like an agent or script node: top, right, bottom or
  left, with preview, saved anchor and a route perpendicular to the side. The edge still
  carries the `result` port.
- An edge to End saved without an anchor keeps the declared side of `result`: existing
  pipelines reopen identically. The merge node stays fixed on its `branches` badge.

## 1.101.0
**"First pipeline" tour: gesture from the edge, port fix in the Outputs section.**

- The edge steps no longer talk about a dot or a handle: they teach the card's edge.
- The tester's two edges advance on the output carried. If the edge does not carry `out`, the
  card re-aims at the edge then at the panel's Outputs section, and advances when `out` is
  checked; the selection step that follows is then already satisfied.
- `when` conditions are accepted on a multi-port edge (`out.verdict`) like on a single-port edge
  (`verdict`); a wrong value holds the step with a precise hint.

## 1.100.0
**Wiring edges from the side, grid-based routing, snapped segment drag.**

- Creation: output dots disappear. An edge starts from any point on the side of the source node
  and is routed by cells of the wiring grid. `Shift` frees the route; releasing it re-origins
  the grid on the current point. The created edge is `mode: manual` and carries the first
  declared output.
- YAML: two layout fields, `source_anchor` and `target_anchor` (`{ side, offset }`), say where
  the wire leaves and touches the cards. When absent, the wire starts from the middle of the
  side as before; they are ignored by the semantic diff.
- Segment drag: snaps to the grid, `Shift` frees it, the other waypoints do not move, two
  aligned segments merge. "Re-route automatically" gives an automatic route aligned on the grid.
- Right-clicking a segment handle no longer deletes a waypoint; the "Delete edge" menu stays on
  the edge.

## 1.99.0
**Movable edge labels, output labels toggle, edge/node drawing order.**

- Output labels: a small tag per carried port, placed alternately at the foot of the edge,
  movable (`output_label_pos`). Per-edge toggle (`show_output_labels`), on by default if the
  source node declares at least 2 outputs.
- Condition label: the `when` pill is movable (`condition_label_pos`) and is placed by default
  next to the line, no longer on the segment handle.
- Drawing order: edges go above nodes by default; the "Draw under nodes" switch (`below_nodes`)
  puts them back underneath, handles and pill included.
- These 4 fields are layout: written only when not at their default value, ignored by the
  semantic diff.

## 1.98.0
**Multi-port edge: a single edge carries several outputs of the same node.**

- YAML: `source: { node, ports: [a, b] }` for a multi-port edge; the single-port `port:` form
  is unchanged and existing pipelines are not rewritten.
- Runtime: a multi-port edge fires once and the downstream node receives one input per port,
  named after the port (`PDO_INPUT_<PORT>`, "Inputs" preamble listed per port).
- Edge panel: **Outputs** section at the top (the last checked port cannot be unchecked); a
  `when` condition chooses the port read and its schema resolves per row.
- The semantic diff ignores the order of the carried ports.

## 1.97.3
**Attachment upload failure on a remote instance: the error names the layer.**

- A 413 **without a JSON body** (HTML page of a reverse proxy, nginx `client_max_body_size` at
  1 MB by default) is attributed to the intermediary, with the size sent and the two limits to
  check, never again `POST /runs failed: 413`. A 413 from the daemon keeps its sentence.
- `Failed to fetch` on a `POST /runs` with attachments becomes "Upload interrupted after
  Ns while sending X MB to <host> …", and an upload that stops progressing **times out** (UI
  and CLI: one minute plus ten seconds per MB) instead of staying hung.
- On the daemon side, a multipart cut off mid-stream answers 400 "upload interrupted … while
  reading attachment `<name>`" (or "after field `<field>`") instead of the misleading "missing
  field: input". A test now covers the 413 on `POST /runs`.
- README: "Attachments through a proxy" section.

## 1.97.2
**Guided tour: scroll outside the spotlight, skills dropdown, "Create" at step 3.**

- The wheel in the darkened area of the spotlight scrolls the modal instead of being swallowed.
- Checking a skill closes the skill selector.
- Step 3 of the "first pipeline" tour: "Create" stays unreachable under a blocker until the step
  has lit it; the Enter key makes the text of step 4 branch instead of losing the pipeline.

## 1.97.1
**The default diff of the review pane shows the run's working tree.**

- For a running run, `/runs/<id>/review` and `GET /runs/<id>/diff/structured` without
  parameters compare the fork point to a snapshot of the working tree (uncommitted changes and
  untracked files included, `.pdo/` excluded) instead of `fork → tip`. The ref selectors offer
  "Working tree"; `?from=fork&to=tip` is unchanged. For a finished run, the default coincides
  with `fork → tip`.
- The snapshot also works when the target repository ignores `.pdo/` (`.gitignore` or
  `.git/info/exclude`), and `/refs` and `/diff` fail consistently if the snapshot fails.
  Reading never touches the node's index, status or HEAD.

## 1.97.0
**Tutorial mode shipped on `main`.**

- Integration release: brings together on `main` the `integration/816-tutorial-mode` branch
  (1.94.0 to 1.96.0: home pipeline menu and stable targets, Spotlight and tour engine, *First
  pipeline* and *First run* tours, practice repository, full tour) and the 1.92.0 / 1.93.0
  releases already on `main`. No functional change specific to this version.
- The CHANGELOG recovers the 1.92.0 and 1.93.0 entries, lost during a reconciliation.

## 1.96.0
**The *First run* tour goes all the way to the output, and the full tour chains together.**

- The **First run** tour no longer stops at launch: six reading steps teach the life of an
  interactive node: find the Run in the list, open the node, talk to the agent in its own
  terminal (copy-ready block), release the completion, wait for the node to finish, open the
  output. Each advance is observed on the real Run state that the inspector already loads; the
  daemon still knows nothing about tours.
- **Waiting for a node to finish has no time limit**: an agent takes the time it takes. The
  popover says so, ticks "completion released" / "node finished" live, and lights up the whole
  inspector as a **soft zone**: dotted lines, lighter greying, terminal readable and clickable
  during the wait. A node that fails is an end too: the step names it and asks for a Next so
  that the sentence is read.
- **Full tour**: between two tours, an intermediate card says where the chain stands and offers
  the choice: *Finish here* or *Continue*. A tour that **stops cleanly** (target missing, launch
  refused) still offers the next tour, without being marked done.
- The popover now carries its exit in small print (`✕ quit`) instead of a button, and says in
  one grey line what it is waiting for ("advances once released") when there is nothing to
  click. **Esc belongs to the node's terminal** while it has focus: the tour no longer steals
  the agent's keys.
- New stable targets: a node's port rows (`out` under Outputs) and the artifact modal, which
  becomes an Esc owner (`role="dialog"`).
- A third Happy Path scenario in `docs/test-scenarios/`: the full tour is **proposed** as the 3rd
  Happy Path (human curation at the integration -> develop MR).

Fixes from running the Feature Path on a real instance:

- **A tour starts on a clear scene.** The artifact modal that *First run* has you open at its
  last step stayed mounted while *First pipeline* was starting: its full-screen backdrop
  swallowed every click, the app frozen behind a modal the tour no longer talked about.
  Floating surfaces now declare themselves dismissable (`lib/overlays.ts`) and starting a tour
  closes them, without any of them knowing what a tour is.
- **"Find your Run" is finally displayed.** Start opens the Run's tab itself, so the step's
  condition was true one tick after its entry and the tour jumped from 9 to 11 without showing
  anything. The step now asks for an explicit `Next` and says what has just happened.
- **The end card no longer covers the output.** The last step recenters on the open artifact
  (soft zone to read, popover beside it) and waits for `Next`: the recap only arrives once the
  page has been read.
- **A new node no longer lands on another** (outside this ticket, revealed by the FP): the
  avoidance now compares the cards' footprint, not 30 pixels. Two cards placed 40 pixels apart
  buried the edge that links them, which *First pipeline* precisely asks to click.

Second pass of the Feature Path:

- **A new node stays on screen.** The avoidance grid reasoned in canvas units and ignored the
  viewport: on a new pipeline, which the canvas frames at zoom 2, one step to the right is 440
  pixels and the second card of *First pipeline* was born **outside the canvas**, unreachable,
  with a tour that asked for it. The search now knows the visible area and only answers with a
  place that can be seen; and auto-framing **no longer zooms in beyond 1:1**, so a card has the
  size it was designed for.
- **A tour can show a canvas node.** A card lives in a transformed viewport: scrolling it does
  not move it. The canvas declares itself able to bring it to the center (`lib/canvasReveal.ts`,
  same pattern as `lib/overlays.ts`) and the tour asks it to, once per aim, and only if the
  card is not already in frame.
- **The two edge steps of *First pipeline* name the output** to drag. The tester has two
  handles at that point; an edge starting from `image_list` carries no `verdict`, and the
  condition requested two steps later became impossible to write. The gesture now only counts
  from `out`, and the grey line names the other handle.
- **A refused `Create` says so.** The New Pipeline modal swallowed the daemon's error: button
  with no effect, no message. It now shows the daemon's sentence and, for a name already taken,
  says so in the reader's terms. The *First pipeline* tour then stops on its refusal card
  instead of waiting for a dead button.
- **The `tutorial-interactive` pipeline no longer overlaps**: its three nodes carry a position,
  instead of letting the canvas stack Start under the agent.
## 1.95.0
**Tour *First run* and practice repository.**

- New daemon verb `POST /repos/create`: creates a minimal git repository in a parent folder
  (one commit on `main`, `README.md` and `notes.txt`), returns the same path without a new commit
  if it already exists, and refuses with a named message an existing folder that is not a repository.
  The created repository does not enter the recents.
- The **First run** tour (Settings › Tutorials) is live: on entry it creates (or reuses)
  `/tmp/pdo-tutorial` and the `tutorial-interactive` pipeline, then guides up to Start: Run
  name, explorer open on `/tmp` with the entry pointed at and scrolled into view, pipeline, Default
  profile, the two PDO skills, the prompt, then the launch. A gesture off target does not advance.
- A closing card names the created Run, warns about the harness trust question in
  `/tmp/pdo-tutorial`, and chains into *First pipeline*. A refused Start stops cleanly,
  quoting the daemon's reason, form intact, no Run created.
- Tour engine: a target that disappears transiently (collapsed folder, filter) no longer
  abandons the tour; the Spotlight falls back to the container until it returns.
- Skill picker: clicking a row's name toggles the skill, not only its checkbox.

## 1.94.0
**Tutorial mode: Spotlight, tour engine and *First pipeline* tour.**

- On first launch (browser key absent **and** instance with no Run at all), a **welcome modal**
  offers the full tour, a specific tour or "later". Any of the three
  choices sets the key: nobody is prompted twice.
- During a tour, the **Spotlight** dims the whole screen except the step's target (above
  modals and portaled menus) and absorbs clicks elsewhere. A popover anchored to the hole gives
  the instruction, the `Step n of N` progress, the copyable block for free-form input, `Skip` when
  the step allows it and `Quit tour` always. Esc too, except when a menu or a dialog
  claims it first.
- A step advances on the **actually observed state**, never on a delay; a target missing for more
  than a few seconds stops the tour cleanly ("This step could not be completed") instead of
  waiting. Tour definitions are plain data: adding a tour does not touch the
  engine.
- The **First pipeline** tour builds `tutorial-implement-test` end to end: two interactive
  nodes (implementer in the Run's worktree, isolated tester), their prompts and
  outputs, a `verdict` schema as a `pass`/`fail` enumeration, an `image_list` port, then the three
  edges, two of them conditional.
- New **Settings › General › Tutorials** section: full tour, one button per tour with its estimated
  duration, a check mark on completed tours and a "Reset tutorial memory" link. Check marks and
  the welcome offer live in the browser, like the theme, never on the daemon side.
- The *First run* tour is announced and greyed out: it arrives next.

## 1.93.0
**Per-tab filters, Total/Active/Waiting duration mode, coverage, ephemeral settings.**

- The Stats title bar keeps only the period (and Pricing details on Cost). Each tab
  carries its own **filter band** with its "completed runs only" cohort and its own default:
  on for Performance, off for Overview / Sessions / Triggers and for Cost. Toggling the cohort
  refetches only the tab concerned.
- On Performance, the "exclude user wait" toggle becomes a three-position control
  **Total / Active / Waiting** (default Active). Cards, column header, box plots, labels and
  sort follow the mode, client side, without refetch. In Active and Waiting, an "i" gives
  "n executions out of N declared at least one wait".
- **Breaking on the browser side**: Stats settings are **ephemeral**. No `pdo.stats.*` key is
  read or written any more (the old ones stay inert in `localStorage`); the state lives
  as long as Stats is mounted, reopening resets it. Independent scales are on by default.
  No more deviation badge on the rail.
- Daemon, additive: `GET /stats/performance` carries `wait_duration` next to `duration` and
  `active_duration` at every level (Total = Active + Waiting per execution), plus
  `waited_executions` and `executions` for coverage.

## 1.92.0
**Archiving a parent Run takes its finished child Runs with it.**

- `cleanup_run` archives the Run **and its finished descendants** (children, grandchildren,
  resolved through `parent_run_id`), grandchildren first, through the same teardown path: the
  children's worktrees and run folders are reclaimed along with the parent. Descendants already archived
  are skipped without error; the `200` response lists the `archived_children`.
- A **live** descendant (running, awaiting-user, paused) is never archived in cascade: the
  request is refused with `409` and the `live_children` list, and nothing is touched. Stop
  the child first.
- In the UI, the Cleanup confirmation box counts the finished children that will leave with the
  parent and warns when a still-live child will make the daemon refuse the request.
- `pdo reap`: a reaped parent takes its finished children with it even when younger than their TTL;
  a parent whose subtree is still running stays listed until it settles.

## 1.91.0
**Active duration excluding declared wait, filter by node kind, finished runs only.**

- In Performance, an **exclude user wait** toggle replaces the wall-clock duration with the **active
  duration**: the declared wait (`pdo wait-user`, unreleased completion) is subtracted, for
  nodes as well as for the Infrastructure row, without refetch. Amber `−Xm wait` badge and ghost box
  of the wall-clock duration on the box plots. A `children_pending` wait subtracts nothing.
- Multi-select filter by **node kind** (Standard / Interactive / Orchestrator) on the Node
  rows of the Pipeline and Model axes; a node that is both interactive and orchestrator matches
  both. Under a partial filter, the Pipeline totals and the headline cards read "filtered",
  never a recomputed value; an emptied level shows "No node of the selected kinds at this
  level · show all kinds".
- In the Stats shell, a **completed runs only** toggle (ON by default in the browser)
  restricts the cohort of the three sections to Runs that reached `completed`; Overview shows
  "Errors: 0 (completed only)" with an amber cohort line. The three settings are
  remembered per browser (`localStorage`, `pdo.stats.*` keys).
- A freshly launched interactive node is `running`: the start primitive no longer emits
  a wait at spawn, only the declaration turns it orange.
- Daemon, additive: `active_duration` everywhere `duration` exists in `GET /stats/performance`;
  `orchestrator` frozen in the `NodeStarted` payload next to `interactive` (falling back to the
  pipeline snapshot for history); `completed_only` parameter on `/stats/overview`, `/stats/cost`
  and `/stats/performance`, included in the memo keys.

## 1.90.0
**Median everywhere, zoom levels and independent box plot axes.**

- Everywhere Stats summarizes a distribution, the central value is the **median**: sort and
  labels of the master list, tick of the Performance box plots, cards and cost per execution
  in Cost ("median" replaces "avg"). The mean remains only in the tooltip, as the
  sixth value. Totals do not change.
- The Performance box plots gain a **three-level zoom** (Full, Fenced, Box: axis
  set on the max, the Tukey upper fence or Q3) and an **"independent scales"** toggle
  (each row on its own axis). A "›" / "‹" mark flags an out-of-frame observation
  without drawing it. Zoom and axis are remembered per browser (`localStorage`).
- Daemon, additive: `GET /stats/cost` exposes `median_usd` next to `average_usd`;
  `GET /stats/performance` adds `fence_low` / `fence_high` (observed Tukey fences) to the
  six stats, computed on the daemon side.

## 1.89.0
**Fetch before the cut and warning on the source branch.**

- A Trigger that fires on `origin/<branch>` **fetches first** (bounded to 20 s) then cuts on the
  real upstream tip: a commit pushed and never fetched is still picked up. A
  `skipped-overlap` tick does not touch the network.
- A failed fetch **does not block** the firing: the Run starts on the last locally known state,
  and the `run_started` event carries `source_fetch_error` (`kind` + git's stderr). The Run view
  says so without painting it as a failure: "Started on local state · fetch failed" callout in Info,
  `CloudOff` marker on the primary repository's `branch · sha` line in Repositories.
- In the Trigger form (creation and edition), a **local** branch carries a
  warning: each firing cuts from its local state, which the fetch does not move;
  prefer `origin/<branch>`. The Trigger list and detail do not change.

## 1.88.0
**Lossless fast-forward from the sync popover and source drift shown on the Run.**

- The New Run sync popover offers a **fast-forward** of the local branch when it is
  strictly behind its tracking branch. The daemon verb (`POST /repos/fast-forward`)
  advances the reference *and* the working tree; it refuses without touching anything, with a named cause
  (`dirty_tree`, `diverged`, `checked_out_elsewhere`, `no_upstream`, `up_to_date`), and each
  refusal offers the shortcut "Run from `origin/<branch>`". PDO never merges, rebases or
  pushes.
- The popover rereads the ahead/behind gap on each opening (no implicit fetch) and the
  result card clears on "Fetch again".
- A Run's view shows the **drift of its source** as an `n↑ m↓` chip (Info and Repositories
  tab): commits on the Run's branch since the cut, and commits that arrived on the source
  (`main +x · origin/main +y`); a fetch button updates the remote part. An archived Run turns
  the chip into `unavailable`.

## 1.87.0
**Branch quick pick, fetch on open and sync button.**

- The New Run "source branch" field becomes a VS Code style quick pick: search by
  fragment, local and remote branches grouped, age and subject of the last commit, `n↑ m↓` gap
  chip on each row.
- Opening the modal or changing the target repository triggers a `git fetch` (bounded to 20 s);
  freshness (`fetched N ago`) is read from `.git/FETCH_HEAD`, with no state stored on the PDO side.
- A sync button exposes five states (`behind`, `diverged`, `up to date`, `fetching`,
  `unknown`); a failed fetch does not overwrite the date of the last success and does not block
  the launch. The shortcut switches the selection to `origin/<branch>`, and the Run then cuts
  on the real upstream tip.
- Each secondary repository carries its own freshness (same component).

## 1.86.2
**Effort levels per model.**

- The harness catalog gains a fifth source (RPC): a `--mode rpc` session lists
  the models then queries **the thinking levels offered by each model**
  (`get_available_thinking_levels`). The catalog now exposes `model_efforts`:
  key absent = unknown (fall back to the global axis), key present (even empty) = known.
- The effort pickers (agent profile, instance custom agent, Node Inspector,
  Merge Inspector) only offer the levels the selected model supports; an explicit value
  that became unsupported stays visible but disabled, with a warning.

## 1.86.1
**Outputs inherited from a previous execution.**

- A node's completion commit **no longer embeds `.pdo/artifacts/`**, even if the target
  repository does not ignore `.pdo/` (amendment to an earlier decision record). The rest of `.pdo/` still follows the
  target repository's `.gitignore`.
- `GET /runs/{run}/nodes/{node}/io` marks `inherited: true` an output file older than the
  iteration's `started_at` (or the Run's if the node never started), **or tracked by
  git in the worktree and unmodified** (arrived with the checkout, whatever its mtime;
  a tracked file that the current execution rewrote becomes its own output again); the panel
  badges it "inherited" and **does not show its frontmatter verdict**. A mixed image
  list shows "k inherited" and greys out the thumbnails concerned. The file stays listed
  (the restarted agent builds on it).
- `GET /runs/{run}/artifact` answers `Cache-Control: no-store`.
- The preamble no longer presents as "partial output of the interrupted attempt" a file
  older than the Run itself, nor a file **tracked by git and unmodified**: the
  worktree checkout happening a few ms *after* the Run starts, only the git signal
  keeps out another ticket's report inherited through the source branch. A tracked file that the
  interrupted attempt rewrote is still passed to the restarted agent.

## 1.86.0
**Declared wait, base preamble, children wait.**

- **Declared wait.** A freshly spawned `interactive` node is no longer `awaiting_user`:
  it has the color of a working node. The agent declares its wait with
  `pdo wait-user [--message "<question>"]` (accepted on any node with a live session,
  interactive or not; named refusal `node_is_script` / `node_session_not_live`: tmux is the
  authority, not the projection alone: a pane dead before the sweep ran is refused, exit 3;
  idempotent): node and run turn `awaiting_user`, the message (≤ 100 characters) is the
  reason in the banner and in `awaiting_reason`, **without** `awaiting_reason_code`. The
  `completion_not_released` refusal of a `pdo complete` also declares the wait. Back to `running`
  on a human's **Enter key** crossing the UI's PTY bridge (after at least one
  byte typed since the declaration; arrows, wheel, mouse and `paste_text` do not count)
  (new `node_resumed` event) or on the release of the completion.
- **Breaking: preamble.** The preamble is identical for all agentic nodes: no more
  interactive Completion block nor "Orchestration: creating child Runs" section. The
  toggles only add the `/pdo-interactive` and/or `/pdo-orchestrate` invocation lines
  after the role prompt (byte-for-byte mirror in the inspector). The seeded skill
  **`pdo-interactive`** (locked, `skill_version: 1`) carries the conduct; the `interactive`
  toggle seeds its reference as `orchestrator` seeds `pdo-orchestrate`.
  **`pdo-orchestrate`** moves to `skill_version: 3`: "Waiting for your children" section
  around `pdo run wait`, binding contract moved from the amendment, no more periodic
  polling. The seed mechanism carries a list.
- **Children wait.** `pdo run wait [--all] [--timeout <s>]` from a node session: blocking
  long-poll on `GET /runs/{run}/nodes/{node}/children/wait` (held on the daemon side at most
  25 s, the CLI loops). Exit 0 + one JSON line per child that became terminal
  (`run_id`, `name`, `status`, `reason`, `children_active`); exit **2** on timeout
  (nothing on stdout); exit 1 daemon unreachable or outside a session (`not_in_node_session`);
  no active child: `{"noop":true,…}`. An `awaiting_user` child does not wake the call
  but makes the parent node and its run `awaiting_user` **by derivation at read time**
  (list, detail, children; nothing written to the parent's log; recursive on the tree),
  reason "child run <name> is awaiting you", parent node `awaiting.cause = child_awaiting`
  (its last iteration too, so that the node detail banner shows).
- **Wire.** `nodes.<id>.awaiting` (`cause`, `message`, `since`, `child_run_id`) on
  `GET /runs/{id}`; a run's `awaiting_reason` can now be a declared wait (the
  `awaiting_reason_code` slug remains the only incident marker).

## 1.85.0
**Steering messages in Stats › Performance.**
Third metric `Steering` (messages typed by a human per execution) on the pipeline and model
axes, plus a `Steered executions` card (share of successful executions with ≥ 1 message).
The counter is derived from transcripts (claude, copilot, pi), never persisted: launch prompt
excluded, messages pasted by the daemon excluded via the `[pdo-runtime]` prefix now
set at the tmux chokepoint (`send_message` / `paste_message`). opencode and Infrastructure show `—`
with a reason, never `0`. Eighth "Steering" capability in the harness support table.

## 1.84.1
**Terminal text selection cleared on the first mouse movement.**
With tmux mouse mode (tracking every movement, DECSET 1003), xterm.js reported each
buttonless pointer move to the pty and cleared its own selection on that "input",
making copying nearly impossible. These moves are now stopped in the capture phase as long
as a selection exists; drag, click and wheel still pass.

## 1.84.0
**Run list as a tree.**
Child runs (created via `pdo run create` from a node session) collapse under their
parent: only roots are at level 0, a chevron expands each parent, and a row of
counters aggregates the subtree (finished / failed / stale / running). A global button expands or
collapses everything; the Settings "Child runs" preference (local to the device) sets the default
state. The pipeline filter keeps the parent of a filtered child; an archived parent becomes a
leaf again and its children move up as roots. The orchestrator node and its Orchestration tab
show an orange badge for "stale" children.

## 1.83.0
**Files and images passed to the child run.**
`POST /runs` multipart accepts a repeatable `files` field next to `images`: any file lands in
`_input/` of the child worktree, the input node sees it under `## Input Files` in its preamble and on its
input port. A duplicate name is **refused** (named 400) instead of silently overwriting. `pdo run create`
gains `--input-file`, `--image` and `--file` and switches to multipart as soon as an attachment is present.
The "New run" modal replaces the "Images" field with "Attachments" (thumbnails + chips, counter, budget).
**New setting** `max_attachments_mb` (default 50, env `PDO_MAX_ATTACHMENTS_MB`): a single budget per run,
shared between the `POST /runs` body limit and the client-side guard. The seeded `pdo-orchestrate` skill moves
to `skill_version: 2` and fixes its examples (`--pipeline`/`--project`/`--variable` did not exist).

## 1.81.0
**Deployment behind a reverse proxy.**
`pdo daemon` and `pdo service install` now accept `--bind <ip>` or `PDO_BIND`.
The explicit address survives updates, the service units deduplicate their `PATH` and
state that harnesses use the interactive shell's `PATH`. The README groups
the `pdo` commands in a table and documents the persistent systemd drop-in.

## 1.80.0
**Interactive node: "Mark ready for completion" hands completion back to the agent.**
On an `interactive` node, `pdo complete` run by the agent is now **refused** (exit 3, guard
`completion_not_released`) until the user has released completion: the agent is invited to ask for the click.
The node panel offers two buttons side by side: **Mark complete** (takes the artifacts as they are, completes
immediately: behavior unchanged) and **Mark ready for completion** (`POST /runs/{id}/nodes/{node}/release`,
command `release_node_completion`, event `node_completion_released`, idempotent). Once released, the node goes back to
`running`, the Run is no longer `awaiting_user`, and the agent completes when it is done. Release is
refused (409 `run_not_live`) without a live session.
## 1.79.0
**Review page: comment carry-over / outdated, Review quick access, Repositories tab.**
When the displayed pair has moved since a comment was written (node redelivery, merge-back), the daemon
**maps the anchor at read time** (`GET /runs/{id}/review/comments?from=&to=` adds `outdated` / `mapped_line` / `moved`
and returns `from_sha` / `to_sha`): unchanged line -> comment **carried over** to its new position (chip `↳ from R…`
when the number differs); edited or deleted line -> **outdated**, collapsed at the top of the file with its original hunk,
still replyable / resolvable. Nothing is written to the event log: the original anchor remains the truth, and
`pdo review list` (fork -> tip) exposes `outdated` to the manager. `Show outdated` toggle (remembered per browser).
Canvas toolbar: the "Run repositories" button goes away, replaced by a **Review** link with a badge for pending
comments (outlined blue = sent unresolved, solid blue = unread reply, amber = resolution
proposed), on every non-archived Run. The `RunInfoSidebar` sidebar is removed: **Repositories** becomes a tab
of the Run panel (`Info | Diff | Repositories | Manager | YAML`), the Info header carries the failure / wait reason
and the edit note. Covers the second half of an earlier ticket (left open).
## 1.78.0
**Review page: conversation with the agent.**
A sent comment becomes a **thread**: the agent replies with `pdo review list [--state open|resolved|all]`
and `pdo review reply <rc-id> --text "…" [--resolved]` (author inferred from the session: `manager` or the
node id). The `review_comment_replied` / `resolved` / `reopened` events are now emitted and projected
(`proposal_pending`, `resolved_by`, `reopened_by`, `proposal_declined`). By default, an agent's `--resolved` is only a
**proposal**: the card turns amber and the human decides with **Resolve** /
**Reopen** (Reopen on a proposal = refusal, the comment stays open). New instance setting
`review_agent_can_resolve` (`PUT /settings`, env `PDO_REVIEW_AGENT_CAN_RESOLVE`, default `false`, checkbox
Settings › Runs "Let the agent resolve review comments directly"): when active, the agent resolves
directly and the human can reopen. Endpoints `POST /runs/{id}/review/comments/{rc}/reply|resolve|reopen`
(idempotent, `changed: false` without event), `GET …/review/comments?state=`. UI side: state icons
(open / proposed / resolved) and author icons, resolved cards collapsed to one line, bell badge for
unread replies (remembered per browser), eye toggle to hide resolved, blue badge on the Run's
Diff tab as long as a reply is unread. The manager's builtin prompt documents the CLI.
## 1.77.0

**Review page: inline comments, drafts and send to the manager.**
The `+` widget on a diff line opens an editor under the line (Write/Preview markdown, ⌘/Ctrl+Enter,
Esc, Cancel · Send now · Save draft). A **draft** lives in the browser (`localStorage` per Run,
in-progress text in `sessionStorage`), editable and deletable, marked with an amber dot on the line.
Sending (per card, via the toolbar badge or the "Send all" footer bar)
writes one `review_comment_sent` event per comment (ids `rc-001…`, shared `batch_id`, ref pair
and SHAs), projected into `RunState.review_comments` (absent when empty: historical payloads
unchanged), and pastes **a single message** into the manager's pane (started on demand) via
`load-buffer` + `paste-buffer`. A sent comment becomes immutable and shows `rc-NNN · pair ·
Awaiting manager reply`. New endpoints `GET /runs/{id}/review/comments` and
`POST /runs/{id}/review/comments/send`; Run branch gone or Run archived -> `409 run_branch_gone`,
sending disabled with the reason, nothing is written. The `review_comment_replied` /
`resolved` / `reopened` events are defined but not emitted yet. The manager's builtin prompt gains
a "Review comments" section. Navigation between comments: `c` / `C`.

## 1.76.0

**Review page: GitHub-like diff review.** From the Diff tab,
"Expand and comment" opens `/runs/<id>/review`, a full page (new tab, reloadable,
ref pair in the URL). It lists the files on the left with their +/−, renders the diff **side-by-side**
by default (unified toggle remembered locally) via a third-party component (`@git-diff-view/react`), and
lets you **expand the context** hidden between hunks from the file's content at a ref. A
**source -> destination** picker offers the Run's refs exposed by the new `GET /runs/{id}/refs`:
fork point, tip, and for each delivered node its `before`/`after` labeled by name and iteration;
default fork -> tip, unknown ref -> fall back to the default with a notice. The panel of a delivered or running node
gains the "Review this node's delivery" shortcut. No "node diff" surface; comments
will come in a later ticket.

## 1.75.0

**Run-level Diff tab that works.** A Run's diff is
now computed in the Run's **effective repository** (the worktree's target repo), no longer in
`state.repo_root`: on a target repo distinct from the daemon's cwd it rendered "No changes". The daemon
exposes `GET /runs/{id}/diff/structured` (files, hunks, +/− counters, fork -> tip refs) and
`GET /runs/{id}/file?path=…` (guards: path outside the repository -> 400, ref/option -> 400, unknown -> 404).
UI side, the Diff section of the Info tab becomes a dedicated **Diff tab** (`Info | Diff | Manager | YAML`):
sticky summary, file ledger, flat transcript with per-file collapse persisted across tabs,
intra-line highlighting. An **archived** Run shows "Diff not preserved for archived runs" (404 on the API side).
The node picker and the Diff section of the Info tab are removed; the *Changes* stat opens the Diff tab.

## 1.74.0

**Page mounts: `pdo page mount|unmount|list` and read-only serving under `/pages/<name>/`.**
An agent or an operator binds a name to a
directory on disk and opens it right away in the browser as real HTML (MIME type guessed, scripts
executed); the content served is the disk's at request time, with no remount or rebuild. The same three
operations over HTTP (`GET|POST /pages`, `DELETE /pages/{name}`), mounts persistent across restarts.
A mount made from a node session carries the Run's id and disappears when it is archived; outside a Run
it lives until unmounted and goes into the `audit_log`. Missing file under `/pages/` = real 404
(never the SPA shell); paths canonicalized and confined to the mounted directory; `.pdo/artifacts/` cannot be
mounted. Storage lives in a new `page_mount.rs` file of the daemon crate (standalone module:
table + queries), hence the layout ratchet.
## 1.73.2

**The daemon startup banner announces a browsable URL.** `pdo daemon` always listens
on `0.0.0.0:<port>`, but a browser opened on `http://0.0.0.0:<port>` sends an
`Origin` that the WebSocket guard refuses: half-loaded UI, "reconnecting…" footer.
The log line keeps the bind address for the operator and adds `— open http://localhost:<port>`
(unspecified bind `0.0.0.0`/`::` -> `localhost`; concrete bind announced as is).

## 1.73.1

**Stats › Cost: a pipeline's headline no longer aggregates Infrastructure/Unassigned cost.**
Only `Node` contributions feed the pipeline/project headline and the pipeline level of the
model axis; the Infrastructure/Unassigned rows stay listed and drillable, and the instance
total still counts every contribution (the earlier decision unchanged): the sum of the rows can
now exceed the headline, which is intended.

## 1.73.0

**Stats › Performance "By model": context peak and duration per model.** The second grouping select ("By pipeline" / "By model") is independent
of the sort ("By context" / "By duration"). In "By model", the Model -> effort -> Pipeline ->
Node tree carries the median context peak and the median duration with the same six-statistic boxes,
`measured` / `expected` / `missing_reasons` unchanged: each session file's peak
is attributed to **that file's model** (a sub-agent has its own), same split as Cost: source
first (`claude` per message, `pi` model + provider + thinking level, `copilot` model + effort at
usage points), falling back to the requested one marked "?" otherwise, nothing
invented when neither speaks. In "By pipeline", a Node's drill ends with
its model × effort pairs (distinct peaks and durations side by side); the Infrastructure row stays
kept on this axis and out of the model axis. Provenance in a tooltip as in Cost. Additive wire
(`by_model`, `models` on Node leaves); Performance memo invalidated by the same
fingerprints (events, mtimes): nothing materialized.

## 1.72.0

**pi and copilot sources: observed model and effort, cross-harness merge by verbatim id.** The harness capability
extends the observed identity source beyond `claude`: `pi` carries model and provider on each message and reads the thinking level
from the level-change event; `copilot` (verified in 1.0.83) logs the
chosen model and reasoning effort at session opening, then the model on each usage point.
Observed effort takes precedence over requested effort, provenance stated in a tooltip ("?" mark
reserved for requested/mixed); the same model id launched through two harnesses makes **one row** with two
harness columns, totals added up; the provider only appears in a tooltip, never
in the identity. A pinned alias and a dated observed id stay two rows. `opencode`, with no cost source,
stays absent from the "By model" axis and `—` on the other axes.

## 1.71.0

**Stats › Cost "By model" for claude.** The
"Cost grouping" select gains the "By model" option: master/detail drill Model -> effort -> Pipeline ->
Node reusing the existing one, verbatim model ids (a pinned alias and the observed id stay two
rows, never falling back to a family). On the backend, the harness capability
gains an observed identity source: `claude` reads the per-message model from transcripts
(cost split per message, a two-model session costing and counting in each bucket),
while `pi`/`copilot`/`opencode` fall back to the model requested at `node_started`; the cost fold
carries model × effort slices (effort "not set" in italics when neither requested nor observed)
and the memo stays intact. The `/stats/cost` wire grows additively: `by_model`,
model × effort pairs as Node leaves on the By pipeline/By project axes, provenance
`observed | requested | mixed` per bucket ("?" mark, the words living in tooltips,
never the word "real"), per-bucket coverage and `by_period` at every level. Title
"per execution" on the model axis and at the Node level, "per Run" elsewhere.

## 1.70.0

**"Orchestrator" toggle + Orchestration tab + badges.** An agent node gains an "Orchestrator" toggle in its edit detail (under
"Interactive", frozen at spawn, immutable during the run) that does four things: it
sets the `pdo-orchestrate` skill reference on the node (visible in the picker, counted in the
semantic diff, removable with the existing "skill missing" warning); it makes the
NodeRun prompt amended at spawn with a `/pdo-orchestrate` invocation line and the
Orchestration contract block, rendered after the role prompt, never in the textarea (toggle and
text cannot diverge), toggle off being byte-identical to before this change; the runtime preamble
documents `pdo run create` among the CLI capabilities (universal, predating the toggle). In a run, the
orchestrator node's detail panel splits into two tabs "I/O" (default, content
unchanged) and "Orchestration": the node's children list via `GET /runs/{id}/children`
(status, started, ticking duration, cost), a click on the title opening the child run with a
"← Back to Orchestrator" bar that returns to the parent on the Orchestration tab, and
green/red/blue badges with counters (finished/failed/in progress; an `awaiting_user` child counts
as blue) in the tab header and on the card in the canvas, hidden while there are no children.
Nodes without the toggle stay strictly identical.
## 1.69.0

**The Pipeline Manager becomes opt-in.** There is no longer a systematic `pdo-mgr-*` session: a run is born
without a manager, and the Manager tab appears on every live run with an empty state ("No manager
on this run", a "Start manager" button, a link to Settings). The session starts on demand
(`POST /runs/{id}/manager/start`, idempotent) and stops with a two-step confirmation
(`POST /runs/{id}/manager/stop`). Two instance settings join Settings > Agents >
Pipeline Manager: `manager_enabled` (auto-spawn when a run is created, using the stored -> env -> default
machinery of the decision record, seam `PDO_MANAGER_ENABLED`) and `manager_profile`, which pins the
manager to an agent profile (harness, model, effort, overriding the run's tier) with a warned fallback
to "Follow the Run" if the named profile has disappeared. The stored value is never
rewritten, and a PUT naming an unknown profile is a 400. `has_manager` is an observed fact
(a `tmux has-session` probe on every `GET /runs/{id}`), never a projected state: a manual stop,
the orphan sweep and a daemon restart cannot desynchronize the UI. Name reservation
now goes through `ManagerStarted` / `ManagerStopped` events (see the decision record), emitted before the tmux
spawn on every path, automatic or manual.

## 1.68.0

**Strong link between orchestrator and children.** A node
carrying the "Orchestrator" toggle (set by an earlier change, frozen in the `node_defs` snapshot of
`RunStarted`) can no longer complete while its child runs (`parent_run_id` +
`parent_node_id`) are non-terminal: `pdo complete` is refused with a 409
`children_pending` and a counter (active/failed; exit 3 recoverable as long as no child has
failed, exit 4 as soon as a child is `failed`). On refusal, a `NodeAwaitingUser` marker
(`reason: children_pending`, idempotent) parks the node and arms a detached watcher that, as soon as
all children are terminal without failure, completes the node through the common path (source
`children_settled`: delivery, validation, terminal event and advance identical to a `pdo complete`).
A child that is `awaiting_user` or `paused` also holds the node; user arbitration (retry,
`Mark complete`) or the `mark_node_done` override, deliberately not gated, resolves it. No
death propagates downward: the link only reads. Stopping, archiving or forgetting the
parent does not touch the children, a restart re-adopts the live children, and a node without the
toggle is never held.

## 1.66.0

**`pdo run create <pipeline>`: create a child run from a node session.** A new CLI subcommand, a thin client of
`POST /runs`: every creation field becomes a flag (input, variables, skills, agent_choice, harness, sandbox,
target_repo(s), source_branch, name, auto_name, auto_fail, provisioning). Identity travels with the session,
never in the body: launched from a node session (`PDO_RUN_ID` / `PDO_NODE_ID`), the
subcommand creates a run mechanically linked to that run and node; outside a session, the run is a root and
`--target-repo` becomes mandatory (readable refusal from the daemon). The child's Project
defaults to the parent's. Daemon refusals come back as is on stderr with a non-zero exit code.

## 1.67.0

**The run list makes the "orchestrated" provenance visible and navigable.**
Every child run carries a neutral badge (GitFork) next to the trigger badge: accent/Zap for a
trigger, grey/GitFork for an orchestrated child, and both coexist. An orphan (forgotten parent)
stays visible but dimmed and inert. The badge is clickable: it opens the parent run
in the view and highlights it (`stopPropagation`, scroll-into-view). A fourth
"roots only" filter axis (icon-only GitFork chip, ON by default, offered only if at least one
orchestrated run exists): children disappear, the filter applies before the active/archived
split, and the "Clear filters" cross resets the toggle. All of it is view computation on
`parent_run_id`: no new endpoint, symbols only in the list, words in the
`title` attributes (tooltips counted: trigger name, total number of children).

## 1.65.0

**A run created from a node session carries its provenance mechanically.**
`POST /runs` reads the session identity (headers `X-PDO-Session-Run-Id` /
`X-PDO-Session-Node-Id`, fed by `PDO_RUN_ID` / `PDO_NODE_ID` in the sandbox) and, after
checking that the node session is **live**, freezes `parent_run_id` + `parent_node_id` in
`RunStarted`. Provenance is never declared: any parent key supplied in the body
gets a 400 as an unknown field, and an invalid claim (unknown run, unknown node, node without a
live session) gets a 403 before any effect. Without a claim, the run is a root; the child's Project
defaults to the parent's, explicitly overridable, with no special sandbox/harness inheritance.
`RunState` and the run list project both fields, and the new endpoint
`GET /runs/{id}/children` groups children by parent node with status, `started_at`,
`completed_at` and cost **derived at read time** through the existing cache (never persisted;
unavailable means the key is absent, never `$0`). A child remains an ordinary run: it opens in the
editor with its own graph.

## 1.64.0

**The `pdo-orchestrate` skill is seeded in the Skill bank.** First seeding mechanism of the product:
at every daemon start, the skill is written into a "PDO" folder, created if absent,
content rewritten if the copy versioned with the binary differs, re-seeded if the
disk folder is deleted or corrupted. It is locked: any write through the bank
(edit, rename, delete, files) answers 403 `locked`, the surface shows a
"Managed by PDO" badge and hides the editing gestures. It remains selectable and deliverable like
any skill. The layout ratchet is realigned on main (drift present since an earlier
change, no new root file added here).

## 1.63.1

**The Settings page closes again.** `SettingsSurface` and `StatsModal`, two
siblings always mounted in `App`, shared the same React key (`0`). React 19
(`mapRemainingChildren`) indexes its remaining children by `fiber.key` alone, so the second
overwrote the first and the opening update landed on the wrong fiber: the
surface became permanently impossible to close (cross / Cancel / Escape dead, DOM frozen). Namespaced
keys (`settings-N` / `stats-N`) plus a regression test `App.settingsClose.test.tsx`
that mounts the real `App` and covers the three closing paths (cross after a Stats
cycle, Cancel, Escape).

## 1.63.0

**Story delivered:** `pi` becomes the third first-party harness. Merge of
the integration branch (four sub-tickets) into `main`. The four sub-tickets below
had been numbered 1.58.2 -> 1.62.0 on the integration branch while `main` published the
1.59 to 1.61 line (in-app update): they ship together in 1.63.0.

### `pi` staging set

**`pi` staging set filled at node spawn.** The sandbox container is born with the first-party
harness homes mounted **empty** (`~/.pi/agent`). A harness's set
(auth, settings, catalog, extensions, skills, never `sessions/`) is copied **once per (Run,
harness)**, at the spawn of the first node that resolves it, behind a marker `<staging>/sets/<harness>`. A
claude-only Run therefore never ships the `pi` auth. The set's env (`PI_SKIP_VERSION_CHECK`, `PI_TELEMETRY`) travels
in the `docker exec`; `pi` sessions are brought back to the host at merge-back under the same
encoded folder, and cost/context reads the staged sink while the Run lives. At a sandboxed spawn, PDO runs a `which`
of the binary in the container: if absent, one WARN per Run and the node goes **Interrupted** with the reason
`harness_binary_missing` (PDO does not provide the image, add `pi` to the profile).

### Instrumented `pi` harness

**Instrumented `pi` harness.** The embedded `pi` descriptor fills the
capabilities left as `None` by the first-party descriptor change: **transcript by identity** (JSONL `*_<session_id>.jsonl` in the
folder `~/.pi/agent/sessions/<encoded cwd>/`), **reported cost** in dollars (sum of assistant `usage.cost.total`,
constant 1.0, shown without `~`, `—` if a message cannot be priced), **context** (peak of
`usage.totalTokens` read against the catalog window) and **turn end** through an `agent_settled` extension
injected into the `{settings}` hole (`-e <file>`), which runs `pdo complete --auto` when
`autocomplete_turn_end` is checked, with no forced exit of the TUI so resuming stays possible. The sweep keeps the
session tail as a fallback. The cost of a mixed Run is broken down by harness (`by_harness`, slices
`derived`/`reported`). Support table and README prerequisites (reachable `pi` model catalog) updated.

### First-party `pi` descriptor

**`pi` harness, first-party descriptor** (the decision records on the harness tiers are amended).
`pi` joins the embedded tier (`claude`, `opencode`, `copilot`, `pi`): a resident TUI in its pane
(`exec pi -a … --session-id <id>`), pre-approved project files, session identity imposed by PDO,
resume **by identity** only (no blind resume). The catalog reads `--list-models`
(`provider/model` models with a context window, served in `GET /settings.model_contexts`) and the
`--thinking` line of `--help` (efforts). The probes declare `None` for cost, transcript, turn end,
limit anchor, staging and context: the support table gains the `pi` 0.85.1 column. The `{settings}` hole
carries `-e` and stays empty until the instrumentation change; the `Stop` hook JSON is now only written for a
harness whose turn-end substrate is the claude transcript. README prerequisites: `pi` auth done
on the host, binary in the user PATH.

### `StagingSet` prefactor

**Sandbox prefactor: `StagingFloor` becomes `StagingSet`.**
The staging capability now returns, per harness, a set (`$HOME`-relative entries, exclusions, env,
*transcripts* entries) plus autonomy fixups; merge-back walks the *transcripts* entries of all
sets. `claude` is re-expressed in this form, byte for byte (the five staging guarantees intact);
`copilot` and `opencode` declare `None`. No observable change; the "Sandbox staging set" row
of the support table replaces "staging floor".

## 1.61.1

**Story delivered:** the in-app update integration branch (three sub-tickets)
joins `main`. The HP suite was not replayed for this merge (human decision); the systemd check of the
stable path (`systemctl --user cat pdo` points to `bin/pdo`) remains manual.

## 1.61.0

**In-app update: detached executor.**
An **Update** button appears in the "What's new" modal and in Settings > Version & update
when a newer version exists (Homebrew or script install method; disabled with a
reason if it is unknown). After confirmation (command shown, restart mode, active Runs,
survival of tmux sessions), `POST /update/apply` launches a detached executor that updates, stops
then restarts the daemon with its current arguments (or through the systemd/launchd supervisor, stable
path of the unit). The UI waits, reconnects and reloads on the new version; the attempt
(status, versions, log) can be viewed in Settings through `GET /update/attempts/{id}/log`.

## 1.60.0

**"What's new" modal on clicking the version.**
Clicking the version in the status bar opens a markdown modal listing the notes of the
missed versions (most recent first, pre-releases excluded, links to the GitHub releases),
served by `GET /update/changelog`. If the check is disabled or the source unreachable, the modal
reports the fallback and shows the `CHANGELOG.md` embedded in the binary; when up to date, it says so and
shows that same changelog. The footer reminds the manual update command (one-click copy).

## 1.59.0

**Version check by the daemon and a read-only "Version & update" section.**
The daemon queries the latest published release (configurable source, cache, check can be disabled)
and exposes `GET /update` / `POST /update/check`. The footer carries a "-> x.y.z" badge when a
newer version exists; it opens Settings > General > "Version & update", which shows the
installed version, the latest published one, the date of the last check, the install method and the manual
update command. No automatic update: read-only.

## 1.58.1

**Delivery of the Settings revamp story:** merge of its integration branch
(two sub-tickets) into `main`. No functional change compared to 1.58.0.

## 1.58.0

**Settings: Agents and Sandbox & worktrees categories as inline sections.**
Agents exposes Harness & models, Agent profiles and Skills; Sandbox & worktrees exposes Default sandbox,
Staging profiles and Worktree provisioning. The former "Manage…" panels are mounted inline and keep
their own persistence ("saves as you go"), outside the Save of the instance form. The former centered dialog
and the drill-down (back chevron) are removed. "Manage staging profiles" in New Run
opens Settings directly on Sandbox & worktrees > Staging profiles; New Run sees a created profile or
a modified default without a reload. Deliberate exception: the Skill bank stays a wide drawer opened
over the Settings (Esc returns to it). Happy path 2, step 13, updated.

## 1.57.0

**Full-window Settings.** Settings open on the same shell as Stats: a rail of four categories (General, Agents, Sandbox & worktrees, Diagnostics), a column of
subcategories by anchor, a single Save with a draft indicator per category and a confirmation on
close if changes are not saved. No setting disappears; the single-tab mode stays
persisted per device under General > Interface. Agents and Sandbox keep their existing panels
("Manage…") pending the second ticket.

## 1.56.0

**Skills travel with the document.** Exporting a pipeline whose
nodes select product skills produces, next to the YAML, a **sidecar** `<pipeline>.skills.zip`
(entries `<pipeline>.skills/<id>/SKILL.md` + reference files): a *Skills (N)* button in the bar
of the portable document, endpoints `GET /pipelines/{id}/document/skills` and
`GET /runs/{run_id}/pipeline/document/skills` (204 without skills). Import (*load files…* accepts YAML +
zip; `POST /pipelines/import` takes a base64 `skills_sidecar`) recreates in the bank the skills whose
id is unknown, **with the same id**, in a root folder *imported with <pipeline>*; a known id stays
intact; a name already taken is suffixed `-2`, `-3`… with a warning; an id absent from the bank
and the sidecar produces a warning, never a failure. The response carries a `skills` report
(`created`, `kept`, `renamed`, `missing`, `folder`). The document's YAML does not change.

## 1.55.0

**Delivery of effective skills in the worktree.** When
PDO creates a worktree (the Run's or the sub-worktree of an isolated node), it copies the node's
effective skills into `.agents/skills/<name>/` and puts a relative link `.claude/skills/<name>` per
skill. The content is **frozen at the Run**: an additive snapshot under `.pdo/runs/<run>/skills/`, outside the
worktree; editing the bank after launch changes nothing for the following nodes. Delivered
paths are excluded from versioning at skill granularity (`# pdo <run>` lines in the target repo's `info/exclude`,
removed at Run cleanup) and filtered out of the completion commit even after `git add -A`.
A versioned `.agents/skills/<name>` stays intact and tracked; the PDO skill of the same name is ignored and
reported (`skipped_skills` on `RunStarted` / `NodeStarted`, visible in the node inspector).

## 1.54.0

**Skill import from a Source.** From the bank, *+ Add ▾ >
Import from a source…* accepts a GitHub repo URL (root, branch, `/tree/<branch>/<path>`),
an SSH URL or a local folder: PDO does a shallow clone with the git credentials of the daemon's user,
recursively scans for `SKILL.md`, validates each frontmatter and presents a checkable list
(invalid ones greyed out with the reason, name collisions to resolve explicitly: replace / rename /
ignore, nothing is written while a choice is missing). Checked skills land in a **Source
folder** named after the Source, which carries its provenance (URL, ref, commit, path); each imported
skill keeps its own, even when moved. *Update from source…* rescans, shows the diff (updated,
unchanged, new at the source, moved out of the folder, gone) and updates after confirmation. Reference
files are copied in full. Endpoints: `POST /settings/skills/scan`,
`/import`, `/settings/skill-folders/{id}/rescan`, `/update`. Schema: skill provenance moves
from `(source, source_commit)` to an object `{url, ref, commit, path}` (additive columns, a 1.51 database
is readable as is).

Touch-ups after the Feature Path: *Update from source* only touches the skills of the targeted
folder (two folders imported from the same repo no longer overwrite each other; a skill from the same
source filed elsewhere is reported as `already in <folder>`); a scan's clone is isolated per
daemon process; a checked "same commit" skill offers replace / rename / ignore instead of
failing at import; the modal warns when a folder of the same name already exists at the destination and
offers to import into it; `Esc` closes the modal after a partial import; the update diff labels
"N reference files changed".

## 1.53.0

**Skills: per-tier selection and effective skills with origin.** A skill is checked at three levels: Instance (settings), Project (project sheet) and Node
(pipeline inspector). A Run launch inherits the first two, with RUN skills
added on the fly (checking a folder checks its skills). A node's inspector shows the list of
*effective skills* with the origin of each (INSTANCE / PROJECT / NODE / RUN) and the list frozen at
spawn (`NodeStarted.skills`). Deleting a skill from the bank lists its referents (projects, nodes);
orphan references keep the id, are shown struck through with a warning (inspector, pipeline lint
banner, New Run modal) and the Run still launches, the skill being ignored
(`missing_skills`). API: `GET /settings/skills/{id}/referents`.

## 1.52.0

**Skill reference files.** A skill can
embed files next to its `SKILL.md`: drag and drop into the paste modal
(files staged before creation, "Create skill + N files") or into the detail's *Files* tab,
multi-select explorer (host path), deletion with inline confirmation, plain-text editing
with explicit save (`⌘S`). A dropped `SKILL.md` replaces the current text (undoable). Limit
10 MB per file, subfolders kept, traversing paths refused. REST API under
`/settings/skills/{id}/files`.

## 1.51.0

**Skill bank.** The instance manages a skill bank from *Instance settings > Manage skills…*: creation by pasting a `SKILL.md`
(frontmatter validated live: kebab-case name, mandatory description; refused without any
disk write), filing into folders by drag and drop, inline rename (case-insensitive uniqueness) and
deletion preceded by an inventory of referents (instance, projects, pipelines, runs).
Each skill lives under `<repo>/.pdo/skills/<id>/SKILL.md`, next to `pdo.db`. REST API under
`/settings/skills`.

## 1.50.0

**Declarative worktree provisioning.** An isolated Run or Node worktree
receives the primary repo's resources that a fresh Git checkout does not contain
(`.env`, `node_modules`, models, fixtures…), declared as `.gitignore` patterns on three lists
(copy, hard link, symbolic link) at the Instance, Project, Run and isolated Node levels. Rules
compose additively; the most specific mode wins; a mode conflict at the same level is refused before
launch. The recipe is **frozen per Run**: a Node restarted on the same iteration keeps its sub-worktree and
its edits. A provisioning error interrupts the Run before any spawn and names the path, the mode and the cause.

## 1.49.0

**A single delivery contract for Agents and Scripts.** *Breaking.*
A Node no longer receives any Git instruction: it modifies files, and the runtime delivers
what it leaves. On completion, PDO keeps the commits the Node made itself, stages the
rest with the semantics of `git add -A` and creates **one** commit `<node-id> iter-<N>: completed`
under the configured Git identity, with no trailer. An isolated Node then has its sub-worktree
merged into the Run's branch; a non-isolated Node has already committed in place. No remaining
change means no commit, so never an empty commit.

This delivery is **a single operation** that the four completion paths go through:
automatic completion (`Stop` hook and turn-end watch), `pdo complete`, manual completion
(`mark_node_done`) and the end of a Script. It runs **before** the terminal event,
so before any downstream start: an isolated Node forked afterwards sees the work of an upstream
non-isolated Node. Manual completion used to do neither merge nor commit, and the asymmetry that
the earlier decision record listed as an accepted limit disappears.

**The `doc_violated_code_immutability` refusal is removed, with all its surfaces.** A non-isolated Node that modifies tracked files is no longer refused: its work is delivered. The slug disappears
from the refusal body, from `awaiting_reason_code`, from the `pdo complete` message and from the client. The
`merge_failed` refusal is replaced by **`delivery_failed`** (`500`), which now covers
staging, commit and merge: it appends a `NodeInterrupted`, parks the Run `awaiting_user`
and **keeps the work on disk**, without undoing or cleaning anything.

**Staging excludes nothing.** PDO removes none of its own runtime paths: the target repo's
`.gitignore` is the only exclusion policy. A repo that only ignores `.pdo/runs/` will
therefore see the blackboard (`.pdo/artifacts/`, `.pdo/prompts/`) enter the delivery commits of
its non-isolated Nodes: ignore `.pdo/` as this repo does.

**New `node_delivered` event** (`before` / `after`, the two heads of the Run's branch),
projected onto `nodes.<id>.delivery`. The **per-NodeRun diff** relies on it: it now exists for
any Agent or Script that delivered changes, isolated or not, and is no longer reserved for Nodes
that owned a `pdo/sub-*` branch. The UI diff selector lists what the NodeRuns
delivered, in addition to what they are. Like the Run diff, it excludes `.pdo/`.

**Sharp-tool stance kept.** Several non-isolated Nodes can run at the same time:
the runtime adds no serialization. The first to complete commits all the non-ignored state
then visible, under its own message: isolation remains the way to get reliable
attribution.

## 1.48.0

**Isolation travels through the library and through import.** A library entry of type `agent` or
`script` now carries `isolated_worktree`, instantiation restores it
as is, and the library preview names the workspace of each entry. Without this, a starred entry
fell back to its type's default on every drop on the canvas: an Agent parked in the Run's
worktree silently forked one of its own.

Isolation also enters the entry's identity. **Consequence on upgrade**: an
entry written before this version says nothing about its workspace; it reads as its type's default
(isolated Agent, shared Script). So nothing moves for most entries, but a Node that
you had taken out of isolation will read `out of sync` against its entry until you
update it. That is the real divergence, not a false positive.

Workflow import: any imported role, annotated placeholder included, becomes an **isolated** `agent` and the
draft writes the line. Import never infers isolation from the prompt, the role name, its
outputs or its membership in a `collection` region.

## 1.47.0

**`agent` replaces `doc-only` and `code-mutating`; isolation becomes explicit.**
*Breaking.* The two former types named an *effect* while the runtime only read a working
directory from them. A single agentic type remains, `agent`, and where the NodeRun
works is written on the Node: `isolated_worktree: true|false`.

The break is clean: **no alias, no migrator, no dedicated diagnostic**. `doc-only` and
`code-mutating` take the path of any invalid value: coercion to `agent`
with the generic unknown-type warning, so a Node that loses its sub-worktree without
saying so. **Existing Pipelines are converted by hand**: a former `doc-only` becomes an
`agent` with `isolated_worktree: false`, a former `code-mutating` an `agent` with
`isolated_worktree: true`. The Pipelines shipped in this repo are rewritten; **those of the
instance (`~/.pdo/pipelines/`) are not**, as they are outside the repo.

The Document always writes the choice for an `agent` and a `script`, even at the default value
(isolated Agent, shared Script): you read where the Node works instead of remembering a default.
`merge` stays isolated by design and exposes no setting; `start` and `end` carry none.
Isolation is **frozen at NodeRun spawn**: an edit moves the next launch, never
a live run, and a resume finds the directory it had left.

On the editor side, the inspector replaces the type selector with a fixed type and a
"Workspace" section that names the two places and shows the resolved directory; the canvas replaces the two
markers `doc-only` / `code-mutating` with a branch glyph on the Nodes that fork a
worktree (the isolated `agent` and the `merge`). Workflow import no longer guesses: an imported role
becomes an isolated `agent`, with no prompt, name or output heuristic.

## 1.46.0

**Pipelines belong to the instance and travel by document.** *Breaking.*
The `repo` / `user` / `library` distinction disappears from all surfaces (list, edit,
save, delete, launch) and `GET /library/pipelines` no longer exists. Pipelines from the
former locations (a repo's `.pdo/pipelines`, user pipelines, `.pdo/library/pipelines`)
are migrated once at boot to the instance registry, collisions renamed, prompts included;
nothing reads the old files afterwards.

Sharing becomes explicit: the YAML tab of a Pipeline, and of a Run, exposes a **portable
pipeline Document** (`pdo_pipeline: 1`) with copy and download, which the import modal
rebuilds into an independent pipeline. The document embeds the graph, positions, notes,
variables and prompts, expands shared nodes into ordinary nodes, and carries no secret,
no environment, no runtime value, no instance configuration: a named agent profile goes back to
**Inherit**. An invalid document or one of unknown version is refused without creating anything.

## 1.45.0

**A single attributed cost fold for the Run, its Nodes and Stats.** The costs of
Claude and Copilot subagents belong to the parent Node and each Claude message is deduplicated
at Run scale. `GET /runs/:id` now exposes a cost derived at read time on every agentic
Node; an unknown value stays `null` and is shown as `—`, never zero.

## 1.37.0

**Library assistant: a single assistant, focus per message, reap on inactivity.** (The decision record
amends the previous one on the assistant's lifetime, spawn and sweep.) No data migration; the SQLite column
`libassist_idle_ttl_secs` is added by an idempotent `ADD COLUMN` at boot, like its neighbors.

The assistant's lifetime unit was the wrong one: one `claude` per pipeline, alive as long as
the **Assistant** tab was displayed. It becomes **the human**: one assistant for the whole
daemon, which learns with every message which template is open, and which dies when nobody
is editing anymore.

Breaking changes on the HTTP API:

- **`POST` / `DELETE /sessions/{pipeline_id}/libassist` -> `POST` / `DELETE /sessions/libassist`.**
  The pipeline id disappears from the path, and the `?scope=` parameter with it: there is only one
  session, `pdo-libassist-shared` instead of `pdo-libassist-<id>`. A client calling the old
  route gets a 404.
- **New `PUT` / `GET /sessions/libassist/focus`.** The UI declares `{pipeline_id, scope}` there, never
  a path, which the daemon resolves itself, and repeats it every 20 s while an edit view
  is open. `GET` serves the same thing as JSON, or as one sentence with `?format=text` (the form the assistant's
  `UserPromptSubmit` hook injects into its context at every message).
- **New `POST /sessions/libassist/save`.** Body `{yaml, prompts}`, **no id, no scope**: the
  daemon writes into the file the focus designates, then broadcasts the `pipeline_changed` that makes the canvas
  reload. It is now the assistant's **only** write path; `POST
  /library/pipelines` remains the UI Library's and is no longer documented to the assistant.
- **`GET` / `PUT /settings` expose `libassist_idle_ttl_secs`** (stored -> env
  `PDO_LIBASSIST_IDLE_TTL_SECS` -> 120 s), the delay after which the sweep reaps an assistant with no
  attached terminal and no edit in progress.
- **`DELETE /sessions/libassist` also clears the focus.** A client that sent `PUT focus: null` then
  `DELETE` can make do with the `DELETE`.

Visible behavior changes:

- **A single conversation, shared by all templates.** Switching from one pipeline to another no longer
  throws away the history. That is the gain; in return there is no more isolation between two
  templates edited alternately, and the assistant relocates itself via the focus at every message.
- **The sweep can now kill the assistant** (`LibAssistIdle`), which the earlier decision record forbade it to do.
  Three verdicts in order: session attached, kept; fresh focus, kept; otherwise killed after
  the idle TTL. This safety net finally bounds the truly broken case: a reload or a
  tab close sends no `DELETE` (React does not run its cleanups on unload), so the
  session survived without bound.
- **The assistant's cwd becomes `<repo>/.pdo/pipelines`** instead of the *library store*. That was a
  bug: a tab of scope `repo` or `user` edits a file of `.pdo/pipelines/`, where the `<id>.yaml`
  announced by the primer did not exist. The real path of the edited file now arrives through the
  focus, absolute.
- **The assistant's save writes into the open file, period.** The word *scope* designates two different
  trees, `.pdo/pipelines/` for an edit tab, `.pdo/library/pipelines/` for the library
  store, and having the assistant carry that word made it write a duplicate in the wrong one, leave
  the edited file untouched and announce "Saved". The argument disappears, and the class of bug with it.
- **Going to look at a Run no longer reaps the assistant** as long as a template tab stays open. "Leaving
  every edit view" is read on the open tabs, not on the active tab.

Accepted degradation: the `UserPromptSubmit` hook only exists on a harness whose launch template
exposes `--settings` (the registry's `claude`; neither `opencode` nor `pi`). Elsewhere, the primer's
equivalent instruction, to go read the focus before acting, is the only mechanism, and the daemon
says so with a `warn!` at spawn.
## 1.35.0

**Run resilience: observability and truthful state** (batch 4/4 of the run resilience spec; related
decision records). No migration: the new state field is additive (`skip_serializing_if`), and
historical logs stay readable (the machine code is derived from the prose prefix when it is missing).

A run's state becomes **self-diagnosing**: the reason for any lack of progress lives in the state,
stall detection is exhaustive by construction, and API inputs tell the truth.

Behavior changes (non-breaking for readers of historical payloads, but visible to API clients):

- **Machine reason plus prose on every lack of progress.** A park / give-up / `Interrupted` / `unrouted`
  now carries an **`awaiting_reason_code`** (stable slug: `session_died`, `spawn_aborted`,
  `boot_recovery`, `run_stalled`, `unrouted`, `region_exhausted`, `region_ended_unrouted`,
  `merge_conflict`, `merge_resolution_failed`, `merge_resolver_spawn_failed`,
  `script_validation_failed`, `frontmatter_retry_exhausted`, `doc_violated_code_immutability`,
  `agent_fail_awaiting`) **alongside** the `awaiting_reason` prose, the same slug+prose contract as a
  refusal. Exposed on `GET /runs/:id` **and** `GET /runs` (the list entry finally carries
  `awaiting_reason`), read by the manager (preamble) and the UI. No more need for `journalctl`.
- **`run_stall_reason` exhaustive by construction.** Waiting on an open region goes through a
  `match` with no wildcard on `RegionStateKind` (loop / foreach / collection): a future region type
  can no longer reopen a run frozen with `stalled=false` (end of that class of bug).
- **Unambiguous `loop_states`.** A bounded region has an entry from **lap 1** (like the legacy `Loop`
  node): "no entry" means "no loop", no longer "first lap" (amends the earlier decision record's
  caveat, carried over into the manager preamble).
- **`POST /runs`: unknown field -> `400` naming the field**, before any effect (JSON *and* multipart).
  No more silent success that discarded a misspelled field (e.g. `target_repos`). Follows the
  validate-on-write doctrine, deliberately narrower than the ignoring of *config* fields (explicit
  validation, no `deny_unknown_fields`).
- **`/commands` error bodies normalized to JSON**: no more `text/plain` swallowed by the front end's
  `.catch(() => null)` (`load_projected`, append failures, `restart_node` probes,
  `cleanup_run`). `{ "error": … }` everywhere.
- **Valid command with no effect -> honest `200 {noop, reason}`** (already carried by the foundation
  through the shared completion gate; covers `skip` of an already terminal node).

## 1.34.0

**Run resilience, batch 3/4: unblock a stuck run without defusing it** (related spec and decision
records, Sharp tool). No migration, historical data readable: the new projection fields
(`region_max_iter_overrides`, `forced_routes`) are derived from the append-only log and are **absent
from the wire** until a command sets them (existing runs serialize identically); the `required:` port
and the `skipped:` marker are also backward compatible.

New steering primitives, all exposed on `POST /runs/:id/commands` (the human forces, PDO obeys, Sharp
tool):

- **`set_region_max_iter(region_id, max_iter)`**: raises the cap of a bounded region **in flight**,
  **absolute** value (unlike the delta of `bump_region`), **uniform** for a literal cap and a `$var`
  cap. The scheduler reads the override (folded from the log) in place of the declared `max_iter`, so
  the region restarts for N laps without editing the YAML or restarting, and the effect holds after a
  reopen.
- **`force_route(from, target)`**: explicit exit from a **node** or a **region** to a target, which
  **bypasses the `when:`** of the edges. The way to lift a run stuck `unrouted` (non-`PASS` verdict,
  green CI, mergeable MR): `force_route <reviewer> -> End` makes it reach Finalize without hand-editing
  the verdict on every iteration. Folded from the log, so **not re-decided** by the `when:` on the next
  lap or after a reopen.
- **`skip_node(node_id, [iter], [overrides])`**: **local skip**: marks a node satisfied with an
  **empty output by default** (overridable per port through `overrides`), and the run **continues**
  (never a `RunSkipped` that would kill everything, unlike `pdo skip`). Counted as satisfied for
  re-projection: a reopen does not re-spawn it.
- **`overrides` on `start_node`**: launches a node with a **dummy input** per port (inline content,
  written as an artifact), without waiting for the upstream. Also covers **emergent** ports
  (`doc-only`/`code-mutating`/`script` nodes with no declared `inputs:`).

And two engine behaviors:

- **Reachability and auto-skip**: a `required:` input (new port field) that is **structurally
  unreachable** (either/or branch not taken) **auto-skips** the node with a **reason** in the event,
  instead of leaving it hanging; downstream advances on the empty output. Bounded sweep
  (`advance_run`) that cascades cleanly (skipping a node -> re-evaluation of a downstream either/or).
- **Richer `unrouted` diagnostic**: the reason carried in `awaiting_reason` **lists the candidate
  edges**, their guard (`when:`/`else`), whether they fired, and **the value actually read** for each
  tested field (`verdict=minor_changes` …). Readable from the run state, without `journalctl`.

## 1.33.0

**Run resilience: recover an `Interrupted` node and a sub-worktree resilient to the environment**
(batch 2/4 of the run resilience spec, related decision records). No migration, historical data
readable (append-only log; a Run from before this change re-attaches or restarts as before).

Recovery of an `Interrupted` node through two mechanisms, triggered by hand:

- **Session re-attach** (optimal): new **`recover_node`** command: resumes the **same** session in the
  existing sub-worktree (`claude --continue`), without restarting the run. **Conditional on a
  declared harness capability** (`HarnessDescriptor::can_resume()`). The resume mechanics (`script`
  exclusion, admission reservation, `NodeStarted` trace) are now shared with `GET …/pane`
  (`reattach_node_session`).
- **Automatic fallback**: if the harness cannot resume, `recover_node` falls back **on its own** to
  **restart-with-artifacts** (pure decision `recovery::choose_recovery`): a fresh agent receives the
  node's partial artifacts as **input**, never overwritten. The spawn preamble exposes the surviving
  partial output (`## Partial output from an interrupted attempt`); it is never wiped on a re-spawn
  of the same iteration.

Sub-worktree resilient to what the agent did to its git (Sharp tool):

- **`classify_sub_worktree`**: a worktree at **our own path** on a branch named other than
  `pdo/sub-*` (the agent ran `git checkout -b feature/…`) is now **`Reusable`**, no longer
  `Occupied`. `Occupied` stays reserved for "the branch is checked out in another **live** worktree".
- **Merge-back follows the real HEAD** of the sub-worktree (`node_tip`), no longer the **name**
  `pdo/sub-*` (earlier decision amended): work committed on `feature/…` enters the pipeline branch
  instead of being lost to a silent "Already up to date".

Already covered by the earlier foundation work, confirmed here: the reap of a surviving `pdo/sub-*`
branch before recreation (end of that bug), and the **inventory** of an interrupted git operation
(`index.lock`, `MERGE_HEAD`, `rebase-merge/`) in the restart response **and** the re-spawn preamble,
never deleted blindly.

## 1.32.0

**Run resilience: the "land on your feet" foundation** (related decision records, several amended).
No migration, historical data readable (append-only log, `resume_run` stays projected).

Behavior change, non-breaking on the wire: **the runtime never forfeits on its own anymore**. An
infra incident (session death, boot recovery, spawn abort) puts the node in `Interrupted` (new node
status, **non-terminal**, distinct from `Failed`) and the run in `AwaitingUser` with a **reason**
(`awaiting_reason`), never `RunFailed`. Runtime give-ups (run-level stall, output validation refusal,
merge conflict, `unrouted`) park the same way in `AwaitingUser`. `Failed` now only comes from a
**deliberate** agent `pdo fail` (opt-in `auto_fail`) or a human abandon.

Notes that cannot be deduced from the title:

- **`auto_fail`**: opt-in resolved `node < Run < Project < instance` (default unchecked): unchecked,
  an agent `pdo fail` parks the run for human confirmation; checked, it ends directly in `Failed`.
  Concerns ONLY an agent `pdo fail`; any runtime give-up parks regardless. `auto_fail` columns added
  (idempotent) to `instance_config` and `projects`; key frozen in `RunStarted` and readable per node
  (`auto_fail:` YAML). Env `PDO_AUTO_FAIL`.
- **Reopen**: `terminal ≠ locked`. `reopen_run` (the **Play** button of the Run-level toolbar)
  re-projects any terminal run (`Completed`/`Skipped`/`Failed`/`Halted`) to `Running`: satisfied
  `(node, iter)` pairs stay frozen (never re-spawned), only unsatisfied work restarts. Targeted
  commands (retry/restart/start/mark-complete/inject) **embed** their own reopen: no more "resume the
  run first", no more re-fail race. The previous terminal label stays in the event log.
- **Idempotent spawn**: a `SpawnOutcome::Failed`/abort on the scheduler path now appends a
  `NodeInterrupted` event naming the node and the cause (end of the run frozen in `running`); the
  reap of a surviving worktree/branch stays handled by `ensure_sub_worktree`.
- Front end: `interrupted` node status (amber), interruption reason in the run sidebar, "run
  finished" action group (Reopen · Retry-all · Open shell) in the canvas toolbar (Variant A).

## 1.31.0

Nothing breaking, no migration. **AI authoring assistant for library templates**
(related decision record). A "Pipeline assistant" Bot glyph appears in the canvas toolbar
**only on a library template** (never on a Run); it opens an **Assistant** tab that attaches a primed
`claude` session, streamed in the embedded terminal.

Release notes that cannot be deduced from the title: the session opens with the **templates folder**
as its cwd (`.pdo/library/pipelines`), not the current pipeline. This is deliberate: neighboring
templates serve as few-shot examples, and canvas<->file is reconciled by the disk-first store on save.
Accepted consequence: opened on a working pipeline with no twin in the library, the assistant
describes an empty folder (an authoring-*from scratch* job). **Create-on-open / reap-on-leave**
lifecycle: the session is created the first time the tab is displayed and **reaped as soon as you
leave it** (tab change or panel close); reopening re-attaches the same session (create-if-absent).

## 1.30.0

**Observable behavior change (security hardening).** The WebSocket `Origin` check extends to the
second endpoint: `/ws` (the dashboard event stream) now **checks the `Origin`** as the PTY terminal
already does. Until now `/ws` had no guard: any page open in the operator's browser could subscribe
to it and passively exfiltrate every event (repos, prompts, verdicts). An `Origin` outside the
allowlist now receives a `403`.

The allowlist is **configurable** through `PDO_ALLOWED_WS_ORIGINS` (comma-separated list of exact
origins), **additive** to the localhost defaults, which is needed behind a reverse proxy / ALB on a
public domain (see the README, *Behind a reverse proxy*). Env-only and never settable from the UI
(the daemon's HTTP is unauthenticated).

Development escape hatch: the Vite proxy already rewrites the `Origin` (`rewriteWsOrigin` in
`vite.config.ts`), so `make dev` works without configuration. For a dev front end served on a
non-standard port, set `PDO_ALLOWED_WS_ORIGINS=http://localhost:<port>`. Bonus: this same fix repairs
the PTY terminal, which had always been broken in Vite dev (same `Origin` cause).

## 1.29.0

Nothing breaking, no migration. **Remote branches enter the source branch selection** of the new Run
form. `list_branches` now lists tracking refs (`git for-each-ref refs/remotes`) in addition to local
ones; the `GET /repos/branches` endpoint returns an array of `{name, kind}` (`local` | `remote`),
local ones first. The release note that cannot be deduced from the title: the chosen source branch is
**stored verbatim** (a remote stays `origin/xxx`), the worktree is **cut directly from the tracking
ref with no `git fetch`** and no materialization of a local branch, the **local twin of a remote is
deduplicated**, the `origin/HEAD` symref is filtered out, and the proposed default is **never a
remote** as long as a local one exists. An unknown source ref is rejected with a 400 (naming branch
and repo), never a half-born Run.

## 1.28.0

Nothing breaking, no migration (schemaless event payload, JSON trigger blob: the field travels as
is). The **secondary repos of a multi-repo Run are writable by default**; `read-only` becomes an
**opt-in per-repo checkbox** (related decision record revising the earlier one). A flag
`read_only: bool` (default `false`) on `RepoPin`/`TargetRepoInput` (`#[serde(default)]` +
`skip_serializing_if`, so a historical pin reads back as *writable*, byte-identical on the wire). The
`secondary_repo_dirtied` guard (409) now only fires on a secondary **checked read-only**; a writable
secondary has its `.git` mounted **rw** in the sandbox (`-v <g>:<g>:rw`, identical path) so that `git`
works there, and its preamble invites the agent to write/commit/ship (env
`PDO_WRITABLE_SECONDARY_REPOS`). **PDO still ships nothing itself**: shipping stays the agent's job
(`Ship It`, `gh pr create` / `git merge`), per repo and independently, and no multi-repo merge-back is
reintroduced. Accepted limit: a secondary made writable **mid-run** only has its `.git` mounted after
the container is recreated (mount frozen at creation, consistent with visibility at spawn).

## 1.27.0

**Breaking, automatic migration.** The **agent harness** (product requirements doc, four slices): the
program that runs a node's agent (`claude`, `opencode`) becomes a four-tier axis (`node -> Run ->
Project -> instance -> claude floor`), resolved **once at spawn** and **frozen** in the start event.
Resume re-applies what was launched, never what the YAML says now. The whole axis ships here: the node
pins, the Run chooses, the Project carries the middle tier, and capabilities are dispatched per
harness.

The tail is no longer composed of hard-coded flags: a **descriptor** carries two argv templates
(launch, resume) and an env block, and a **pure** module (`harness_argv`) renders the string with a
single rule: *a token with an empty hole disappears entirely*. The `claude` tail stays
**byte-identical** when nothing new is set (goldens). `claude` and `opencode` are in the **embedded
floor**; nothing is written to disk.

**Node schema migration**: the flat `model:` / `effort:` fields become a per-harness map
`harnesses.<name>.{model, effort}` (model and effort are read in the winning harness's entry, with no
axis of their own). The pipeline migrator folds them under `harnesses.claude` at startup; a scalar
`pin_harness:` pins a node's harness. The map is **semantic** (diff + `content_hash`). A harness
default **and** a model default **per harness** join the instance configuration (`stored -> env ->
default`, earlier decision amended). A harness binary not found at spawn fails **loudly** (never a
2xx), naming the harness.

**The Run is a tier**: the harness chosen at creation is **frozen in the creation event** (same
immutability posture as the sandbox mode), offered by the launch dialog, and carried by a Trigger
**by construction** (a Trigger's template *is* the payload of a Run creation, so there is no separate
Trigger tier). A node that has **pinned** resists the Run's choice. **Infra sessions** (Pipeline
Manager, merge resolver) follow the Run's harness: "this Run runs on X" becomes true without
exception, including for the unblocking tool, which an A/B test on a new harness therefore also puts
to the test.

**The Project is an entity**: a **named** grouping of repos that are worked on together, and the
middle tier of the precedence. **Nothing is seeded**: as long as nobody has named a group, there is
no Project row and lists are grouped **by the same paths as before**, derived client-side; it is the
pencil on a group header that **creates** the entity, same posture as the price table. A path belongs
to **at most one** Project, compared **verbatim** (no canonicalization), and a conflicting add is a
**refusal naming** the owning Project. A Run's Project is that of its **primary repo**: adding or
removing a secondary changes neither the Project nor the resolved harness. The "only group from 2
on" threshold now applies to **projects**.

**Visible change: the order of group headers.** Groups are now sorted on the **displayed label**
(ties broken by the key), where the previous version sorted on the repo's **full path**. This lets a
Project, whose name has no path, take its place in the same order as the path groups. Consequence to
know: with identical content, a list may appear in a different order than in the previous version,
without any Project having been created.

**Capabilities are per-harness code, and their absence is stated.** The five (cost source, transcript
resolution, end-of-turn substrate, limit-menu anchor, staging floor) go behind a factory
(`harness_probes`) whose **methods all have an "absent" default**: a harness declared as data gets no
capability unless one is explicitly provided, so none can be forgotten. `claude` keeps all five
unchanged. Visible consequences: a Run whose harness has no cost source shows **"-" and a reason**,
never `$0` or a mute `partial` (same vein as `unpriced_models`); the end-of-turn and limit-menu
probes **do not run** without the capability, so no node is auto-completed on an invented heuristic;
a sandboxed Run on a harness with no staging floor **says so once**. This slice also ships the
**disk tier** of descriptors: you declare an unknown harness and launch it without recompiling, by
merging **by name** with the embedded floor, and an unreadable descriptor stays **inert and
diagnosed**: its key falls back to the next tier, never partially applied.

**Out of scope, deliberately**: the sandbox (the staging floor stays specific to `claude`),
`opencode` capabilities (it launches, attaches, resumes and completes through `pdo complete`: no
cost, no automatic end of turn), effort on `opencode` (no effort axis at launch), and the model
catalog (the model stays free text).

## 1.26.0

### Breaking: a failed tmux spawn fails the node and the Run, never `Spawned` anymore

`spawn_node` swallowed the `Err` of `tmux_session_manager::spawn` and returned `Spawned` while
`NodeStarted` was already durable: the node projected as `Running` **with no session**, then the
liveness watch rewrote it `Failed` about 30 s later with a **wrong cause** (`session_died`), and
`restart_node` answered a lying `200 {"spawned":[…]}`. Now the `Err` arm appends `NodeFailed` (legal:
the iteration is `Running`) **then** a gated reap (only the sub-worktree that *this* spawn created,
never a reused one) **then** `RunFailed`, and returns `SpawnOutcome::Failed`. Wire effects (truthful
state): `restart_node` answers **`500 {"error":"spawn_failed","recoverable":false,"run_failed":true}`**
instead of `200 {"spawned":[…]}`; the `resume`/`pause` routes count the node as **`noop`** (with its
reason) instead of declaring it `spawned`. Closes the same-named "accepted limit" of the related
decision record.

## 1.25.0

Nothing breaking, no migration (`CREATE TABLE IF NOT EXISTS`, no backfill). PDO gains a **third
journal**: `audit_log`, the home of **out-of-Run configuration mutations** (Trigger create/patch/delete
and global pause), until now invisible to the `event_log` (whose `Event.run_id` is mandatory). A
dedicated table with no `run_id` in `pdo.db`, written in the **HTTP handler** after commit
(best-effort: under-reporting possible, over-reporting never), read by `GET /audit` (global feed,
descending, filterable by target and `[from, to)` window). The origin is a **declarative and forgeable**
hint (`actor_hint` through the `X-PDO-Actor` header, never a gate: bind 0.0.0.0 with no auth). Closes
the cause of a false alarm: a Trigger turned off by hand now leaves a trace. The *live* `overdue`
signal is deferred beyond v1.

## 1.24.0

Nothing breaking, no migration. **Completion on end of turn gains a primary, event-driven,
agent-side substrate**: a Claude Code `Stop` hook (`pdo complete --auto; exit 0`), injected at
spawn/resume through `claude --settings`, completes the node as soon as the agent finishes its turn;
the daemon sweep becomes the fallback (related decision record amending an earlier one). Opt-in,
**unchecked by default** (`autocomplete_turn_end`), reuses the `NodeAutoCompleted` event with a
distinct label, safe by construction (`; exit 0` swallows refusal 3 for missing outputs, so there is
never a loop), immune on the `script` side (bash, never `claude`). Version bump **re-set to next-free**
(1.24.0 against `origin/main` 1.23.0) after a collision with another change.

## 1.23.0

Nothing breaking, no migration. **A node's transcript is resolved by pinned session identity**
(`<uuid>.jsonl`) instead of the most recent file of a shared project folder: a non-code-mutating node
no longer collides with the transcript of the `__manager__` session. The spawn of an agent node pins a
UUID v4 `--session-id` (never for a `script`), the watch and the resume resolve by that exact name,
and infra sessions stay without an id (so never probed or resumed). Earlier decision amended (tail
byte-identity invariant lifted for an agent node, kept for `None`). Version bump **re-set to
next-free** (1.23.0 against `origin/main` 1.22.1) after a collision with other changes.


Nothing breaking. **A Run's repo list becomes editable mid-Run**: adding/removing read-only secondary
repos, primary locked (slice 2). Version bump **re-set to next-free** (1.22.0 against `origin/main`
1.21.0) after a collision with another change, as planned at the initial bump.

### Mid-run editing of the repo list

Slice 1 froze the secondary list at Run creation. This slice delivers the last explicit need: "the
repo list must stay editable at any time". Purely additive: we stay in the read-only snapshot model
(no merge-back, no `base_sha`, no `commit-tree`).

- `PATCH /runs/{run_id}/repos` (`{ add, remove }`) adds/removes secondaries on a **live** Run, from the
  Run detail panel (primary locked). Typed refusals (`RepoEditRefusal`): `run_not_editable`,
  `secondary_is_primary`, `secondary_already_pinned`, `bad_secondary_repo`, …
- New event `run_repos_edited` carrying the **complete re-frozen active list**; `RunState.target_repos`
  is no longer frozen at `RunStarted`. The reducer overwrites the list and **no-ops on a terminal Run**
  (a doubled guard, together with the handler's 409 refusal).
- **Visibility at spawn**: an edit affects nodes launched **after** it; nodes already alive keep their
  frozen context. An add materializes the snapshot (0 new mounts); a removal leaves it on disk until
  cleanup, now **disk-driven** (sweeps `repos/*`, covers removed-but-persistent and orphaned snapshots).
- The "Repositories" panel stays reachable **while a node is running**: a dedicated canvas toolbar
  button opens the Run detail without the auto-selection of the live node immediately taking it back
  (otherwise "at any time" editing was impossible in the browser).
- Still deferred: the per-node `repo:` selector, in-sandbox `git` on a secondary, and writing / MR in a
  secondary.

## 1.21.0

Nothing breaking. A **purely additive, read-only field**: `GET /stats/cost` now carries a **`resolved`**
array, one entry per family key, with the **winning tier** (`manual` / `fetched` / `embedded`) and the
`$/MTok` actually applied. Rendered in the **Stats -> Cost** tab, next to the **"Sync costs"** button:
you sync prices and read what PDO knows how to price in the same place, and the refetch triggered by
the sync refreshes the table. It is the same `PriceTable` that the cost fold prices with, so the view
can never diverge from what actually prices. **Within the scope of the pricing decision record**: this
slice **amends its out-of-scope section** (which declined a `GET /prices` as redundant with
`manual_keys` + `fetched_rows`): **no new decision record**, **no dedicated route** (additive field on
an endpoint already consumed by the Cost tab, backward compatible, zero Vite dev proxy tax, see
*Versioning*). Bump re-set against `origin/main` (1.20.0) after a bump collision: the `resolved` view
now also reflects the gen-5 floor introduced in 1.20.0 (fourteen embedded families instead of eleven).

## 1.20.0

Nothing breaking. The **embedded price floor** now seeds the current generation
(`claude-opus-5` / `claude-sonnet-5` / `claude-fable-5`), so an **offline, never-synced** instance
prices its default model instead of showing `~$0.00 †` (amends the pricing decision record). The
floor stays a **floor, not a mirror**: a sync still overrides every key (e.g. `sonnet-5` at its live
introduction). Bump set against `origin/main` (1.19.0); to be re-set to next-free if another Run ships
in the meantime.

## 1.18.0

Nothing breaking. A **purely additive** capability: a Run can now read several repos. Bump set
against `origin/main` (1.17.0); to be re-set to next-free if another Run ships in the meantime.

### Multi-repo per Run: read-only secondary repos

A Run could only target **one** repo (`target_repo`). This slice opens **multi-repo per Run** in its
narrowest increment: **multi-repo reading, single-repo writing**.

- At Run creation you select N **secondary repos** in addition to the **primary** (`target_repos[0]` =
  the primary, semantics of `target_repo` unchanged). Each secondary carries a *target branch*
  (default: the **local** ref, not `origin/main`; there is no `git fetch`).
- Each secondary is frozen at a **SHA at start** (`git worktree add --detach`), under
  `<primary>/.pdo/runs/<id>/repos/<alias>/`. Reproducible: mutating the secondary's local checkout no
  longer moves the Run.
- Nodes **read** the secondaries by **absolute path** (injected in the preamble + env
  `PDO_SECONDARY_REPOS`); sub-worktrees do not inherit the snapshot's files.
- **409 `secondary_repo_dirtied`** guard if a node dirties a **tracked** file of a secondary
  (*untracked* files are tolerated); non-terminal (revert + re-completion pass).
- Cleanup: `git worktree remove --force` **+ `prune`** in **each** secondary at teardown (without the
  prune, a phantom `--detach` registration remains).
- Triggers carry the list (`target_repos` column, forwarded at fire).

Deferred (later slices): writing/MR in a secondary, mid-run editing of the list, in-sandbox `git` on a
secondary, multi-repo merge-back (permanently rejected), per-node `repo:` selector, multi-bucket
list/cost grouping.

## 1.14.0

Nothing breaking. A **purely additive** field on the cost estimate (within the scope of the pricing
decision record, no new decision record).

### The cost estimate now names the unpriced model

Until now, a Run (or a bucket of the Stats modal) in which a session had run on a model that no tier
prices showed `~$0 †`, or worse, a plausible but falsely low number, with a generic tooltip "an
unpriced model was excluded". The `†` alone **never said which** model, so `claude-fable-5` (the most
expensive) could stay invisible for weeks in `/stats/cost`.

Now `CostStat` carries `unpriced_models: Vec<String>` (**undated** family keys, sorted, unique);
`partial` is **derived** from it (`partial ⟺ !unpriced_models.is_empty()`). The field travels on
`GET /runs/:id` and, unioned per bucket, on `GET /stats/cost`; the UI names the model(s) in the `†`
tooltip. The cost memo key is **unchanged** (the information travels in the value), so there is no
regression of the `/stats/cost` path.

What is **not** in this version: seeding the embedded floor with gen-5 (opus-5, sonnet-5, fable-5),
which would revise the membership principle of the pricing decision record and is a decision for the
owner. On a non-synced instance, the account's default model therefore keeps contributing $0; the
difference is that it is now **named**, hence actionable (one "Sync costs" click or a line in
`~/.pdo/prices/models.yaml`).

## 1.13.0

**Breaking** (`feat(…)!`): the `stale_git_lock` response field disappears, replaced by
`interrupted_git_ops`. Amends an earlier decision record, no new one.

### `stale_git_lock` (`string|null`) -> `interrupted_git_ops` (`array`, `[]` if nothing)

When `restart_node` reuses the sub-worktree of a `code-mutating`/`merge` node, a session killed
**in the middle of a git operation** leaves markers there in the private gitdir (`index.lock`,
`MERGE_HEAD`, `rebase-merge/`, `rebase-apply/`). The daemon **detected** them, but only reported
**one**, the first of the scan. Chain measured on the 1.12.0 code: an `index.lock` **masked** a
coexisting `MERGE_HEAD`, the agent removed the lock it had been warned about, ran `pdo complete`, and
the merge-back did a `git commit` with the `MERGE_HEAD` still in place -> **a two-parent merge commit
that nobody wanted, silently** (`MergeResult::Success`, zero events). The safety net of the earlier
decision record sits downstream of the commit; it does not catch this.

Three changes:

- **Full inventory.** The scanner now reports **all** markers present, in scan order (`index.lock`
  first), instead of the first alone.
- **Breaking wire migration.** The success body of `restart_node` replaces
  `"stale_git_lock":"index.lock"|null` with `"interrupted_git_ops":["index.lock",…]|[]`: **always** an
  array, never `null` nor absent (a client reads `body.interrupted_git_ops.length` without a guard).
  "Stale git lock" was wrong for three markers out of four: a `MERGE_HEAD` or an interrupted rebase is
  not a lock, and naming it that way hid the second marker behind the first. The field, internal as
  well as wire, propagates from the scanner (`SubWorktreeState::Reusable`) up to the HTTP response.
- **Routing to the preamble.** The differentiated instruction (remove `index.lock` first; inspect then
  finish **or** abort a `MERGE_HEAD`/rebase, at the agent's judgment) now arrives **in the preamble of
  the re-spawned node itself**, no longer only in the body the manager sees. The fresh agent no longer
  waits for it to be relayed, and the daemon **never** deletes a marker (it cannot prove the writer is
  dead, and an earlier incident shows how costly that is).

No frontend work: the UI already discards the response body (`responseMode:"void"`), and the
observability gap remains tracked separately.
## 1.12.0

Nothing breaking. A new **purely additive** instance setting (a direct application of the existing instance-settings decision record, no new decision record).

### Automatic Run naming by the manager is now configurable

Until now, a Run created without a name was **always** named by the Pipeline Manager (from its input, or through a placeholder renamed on a best-effort basis), and Triggers were named with no way to configure it. This release delivers three things, without breaking anything:

- an instance default `default_auto_name` (boolean, resolved `stored -> env PDO_DEFAULT_AUTO_NAME -> default **true**`), exposed in `SettingsModal` with the usual source disclosure. The column `instance_config.default_auto_name` is NULLABLE, with a PRAGMA-guarded `ALTER ... ADD COLUMN` migration;
- a per-Run override: optional `auto_name` field on `POST /runs` (JSON **and** multipart). **Compatibility is preserved**: a caller that passes a `name` without the flag keeps its name exactly as before (the flag only resolves to the instance default when it is absent AND no name is provided);
- a per-Trigger override: column `triggers.auto_name` (`NOT NULL DEFAULT 1`, so existing Triggers keep auto-naming), frozen at creation from the instance default and read at fire time. When disabled, every Run born from the Trigger carries a stable *placeholder name* (`Untitled run <id>`) and the manager is not instructed to rename it.

## 1.9.0

Nothing breaking. One note, because it changes **retroactively** what an operator may believe about past incidents.

### The orphan sweep killed the sessions it had just spawned

The reaper took its two observations in the wrong order: a snapshot of **all** Runs (N+1, 21 s measured on 437 Runs in production) **then** an enumeration of the tmux sessions. A session born in between was alive in tmux and absent from the snapshot, so it was classified as an orphan and killed. In one occurrence, **150 ms after its own spawn**. Nine occurrences in eight days, two Runs lost on the night of 2026-07-30; the probability grew with the number of retained Runs, which made it a defect that worsens on its own.

**What to re-read in your past incidents**: the liveness watch blamed these deaths on tmux, under a perfectly credible `session_died: tmux session pdo-... no longer exists`, followed by a `run_stalled` and a `Failed` Run. Any `session_died` verdict prior to this version on a node that had just started is therefore suspect: the named cause blamed tmux, RAM or the API for a bug in the ordering of observations. The only witness of the kill was `journalctl`.

The order is now carried by the types rather than by the discipline of callers: `decide_sweep` is pure and receives the tmux inventory as an **input** keyed by session, so the reverse order can no longer be expressed; the log is read afterwards, and the proof is by contraposition (the log only grows, so an absence observed *after* the inventory guarantees the absence *at the time of* the inventory). The N+1 disappears by construction: only the Runs that hold a live session are projected. On the spawn side, `start_node` no longer spawns: it returns an intent that the caller executes **after** appending `NodeStarted`, so no session exists before its reservation.

No change to the reap policy: the three orphaning reasons, the shell without TTL, the Manager without TTL and the blindness to `iter` are unchanged, and the eight kill messages are byte-identical. Two observability additions: kills for **absence** and for **unrecognized name** now log at `warn!` (nominal housekeeping, archived or TTL, stays at `info!`), so `journalctl -p warning` finds them without a full grep; and `GET /sessions` carries `reaper: { last_sweep_at, killed, killed_for_absent_run }`. Both counters are **cumulative since the daemon started** (not persisted: a restart resets them to zero), because a kill is an *event*: a per-pass gauge answered "did the *last* sweep kill?", hence `0` however fast you looked, since the killing pass is followed within seconds by an empty pass. `killed_for_absent_run` must stay **flat**: after this fix, an absence observed on a live session is a "can no longer happen", and the cumulative count is what makes that claim verifiable. The per-session detail remains in `journalctl`.

This does not fully close the related leftover problem: the sub-worktree and the branch of a killed session survive and doom the node to respawn; this fix only removes its main producer.

> **Numbering note**: this decision record is **0038**. It was written and reviewed under number 0037, which the 1.8.0 change took in the meantime; the renumbering is purely editorial.

## 1.8.0

A breaking change shipped under a **minor** bump, in line with the previous ones set in 1.2.0, 1.3.0 and 1.6.0: the daily surface is identical (`restart_node` is called in the same place with the same arguments), and the removed behavior was a **lie**: the command answered `200 {"ok":true}` on all five possible outcomes of its spawn, including failure, and on `code-mutating`/`merge` nodes it failed **100%** of the time while claiming to have succeeded. No live configuration can depend on it without already being broken. **If the maintainer prefers the letter of the preamble above ("breakage is announced here and by a major bump"), it is `2.0.0`: one word to change in `Cargo.toml`.**

### Breaking: a `restart_node` that did not spawn is never a `200` again

The arm killed the tmux session, appended its `CommandIssued`, then discovered the Run, the pipeline and the node, and **dropped** the `SpawnOutcome` of `spawn_node`, not even with a `let _ =`. See the decision record on restart outcomes.

| Cause | Status before | Status after | Slug (`error`) | `recoverable` |
|---|---|---|---|---|
| Transition guard rejects (3 reachable reasons) | `409`, prose in `error`, no `recoverable` | `409` | `restart_refused` | `true` |
| `node_id` absent from the pipeline **of the Run** | **`200 {"ok":true}`** | `400` | `node_not_found` | `true` |
| Sandbox container not ready yet | **`200 {"ok":true}`** | `409` | `sandbox_prep_not_ready` | `true` |
| Sub-worktree held by another live worktree | **`200 {"ok":true}`** | `409` | `sub_worktree_occupied` | `true` |
| The spawn failed (`SpawnOutcome::Failed`) | **`200 {"ok":true}`** | `500` | `spawn_failed` | `!run_failed` |
| Session cap throttle | `200 {"ok":true}` | `200 {"ok":true,"waiting":true,"reason":...}` | n/a | n/a |
| Spawn succeeded | `200 {"ok":true}` | `200 {"ok":true,"spawned":[...],"reused_sub_worktree":...,"base_sha":...,"stale_git_lock":...}` | n/a | n/a |

- **Clients discriminate on `error`, never on the status.** Same rule as in 1.6.0, same type pattern (`RestartVerdict` carries no status; its projection is the only owner of it, with an exhaustive `match` and no wildcard).
- **Every knowable refusal is returned BEFORE the session is killed.** An earlier decision record said "validate before writing"; this release extends it to the kill. A `4xx` returned after a session has been destroyed is not a validation, it is a finding.
- **`session_killed` is the new field, and the only one that carries a useful bit on a refusal.** `false` = nothing was touched, fix the cause and call again. `true` = the session is dead and **nothing replaced it** (only two post-kill races): this node needs another lever, not a retry of this one. `recoverable` is uniformly `true` on refusals: its definition (from an earlier decision record) is "has the daemon already recorded the terminal outcome?", and no restart refusal records anything.
- **`Throttled` stays a `2xx`, and is not a `noop`.** A `NodeWaiting` **was** appended, it flipped the node to `waiting`, and the admission sweep really does pick it up again. Calling it a "no-op" would be the small symmetric lie of the one this batch fixes, hence the `waiting` vocabulary, which amends an earlier decision record for this class of commands.
- **`spawn_failed` is a `500`, not a `409`**: it is not a verdict, it is a failure. `run_failed` is **re-projected** after the spawn rather than guessed, because the four producers of `Failed` diverge, and a `500` routes the CLI to `pdo fail`, which is catastrophic advice if `RunFailed` is already in the log.

### Uncommitted work in a sub-worktree now survives a `restart_node`

This is a **new guarantee**, and it cannot be deduced from the title. A node's sub-worktree is named purely from `(run, node, iter)` and `restart_node` re-spawns on the same `iter`: the re-spawn therefore replayed `git worktree add -b <already existing branch>`, which git refuses (exit 255). On `code-mutating` and `merge` nodes the lever failed 100% of the time, silently, and the liveness watch invented a `session_died` 30 s later, a false cause that sent the operator down the tmux trail for a git bug.

The sub-worktree is now **reused in place** when it is there and on the right branch: no mutating git call, the in-flight work of the dead session stays intact, and this is what distinguishes `restart_node` from `node_retry` on disk (`node_retry` remains the clean-slate tool, and the only one that gives a fresh base). A worthless leftover (prunable record, orphan branch) is recycled; a sub-worktree **held elsewhere** is refused, naming what holds it, and never reaped. The success body carries `reused_sub_worktree`, and `stale_git_lock` when a git lock lingers in the worktree (reported, never deleted: PDO cannot prove the writer is dead).

Two fixes that this reuse makes mandatory, because it promotes two latent bugs into systematic ones: `reap_orphan_sub_worktree` now prunes before deleting the branch (otherwise it left both locks in place when the directory had already disappeared; this is also lever 2 of the leftover sub-worktree problem); and `commit_and_merge_sub_worktree_inner` finally checks the **status** of its `git add -A`. The latter held, with a residual `index.lock`: `add` fails (128) -> `diff --cached --quiet` answers 0 -> no commit -> `git merge` says "Already up to date" -> `MergeResult::Success` on **100% of the work lost**, with no conflict, no event, no trace. It now calls `bail!`, and comes out as `merge_failed`.

### End of a restart's self-throttle

At `live == cap`, a `restart_node` on a live iteration throttled **against itself**: the arm kills the session but appends no lifecycle event, so the node still projected `Running` when the count was taken. The freeze was **permanent**: `retry_waiting_nodes` has no timer, `resume_run` is a no-op on a throttled node, boot recovery only looks at `Running`/`AwaitingUser`, and the Stop button answers `409`. The admission count now excludes the slot that the spawn is taking back, on the key `(run_id, node_id, iter)`, never `(node_id, iter)`, which would discard the live session of **another** Run of the same pipeline and **exceed** the cap. And `kill_node` wakes the `waiting` nodes, which nothing did from the commands surface.

### What does not change

- **`GET /sessions` and the status bar gauge** still report the **true** count: only the admission gate applies the exclusion.
- **The liveness watch**: unaffected by construction, since it reads events, never an HTTP status. It simply stops inventing a `session_died` where a `worktree add` had failed.
- **The merge-back adoption rule**: unchanged. Only the way a re-spawn obtains its `base_sha` changes: a reuse **carries over** the original base instead of deriving one, without which the escape hatch would be silently dead (or worse, wrongly armed) for any restarted node.
- **The contract of `node_retry`** and that of the four loop commands.
- **The `text/plain` bodies** of "run not found" and "cannot read/parse pipeline": same bodies, same content-type, only their **position** changes (they now come before the kill). Normalizing them is planned separately.
- **The kill stays bare**, and is not `reap_node_session`: the pane snapshot would never be served on a non-terminal iteration.

### Migration

- **Direct callers of `restart_node`** (`curl`, Pipeline Manager, scripts): replace any test of the form `if resp.ok` with a read of `body.error`, then `body.recoverable` and `body.session_killed`. A `409`/`400`/`500` is no longer a transport anomaly, it is a **verdict**.
- **Treat `waiting:true` as a deferred success**, not as a failure: the node is reserved, it will spawn when a slot frees up. Do not re-issue the command.
- **Read `reused_sub_worktree`** before telling the fresh agent to start from scratch: when it is `true`, the directory already contains the uncommitted work of the previous session.
- **Nothing to do on the data side**: no event payload changes shape. The `base_sha` of a re-spawn `node_started` is now that of the original cut instead of being absent.

### Known gap, stated so that it is not filed again as a regression

This wire truth is **not observable by any human through the UI**. `api.ts` calls `restartNode` with `responseMode:"void"` and a `catch {}`: both the body *and* the error are dropped. The only wired button lives in a banner gated on `node.status === "stale"`, which nothing produces anymore. The live consumer of this route is the Pipeline Manager through `curl`, whose preamble is updated here. The UI part is tracked separately. Zero frontend work in this batch.

## 1.7.0

Nothing breaking. One note, because it cannot be deduced from the commit title and it changes what an operator may assume about their branches.

### The runtime can now move the ref of a pipeline branch

Until now the runtime never touched an existing ref: it created branches and worktrees, deleted none, rewrote none (per an earlier branch-handling decision record). The merge-back of a sub-worktree can now **move** the tip of the pipeline branch to adopt the node's tree, when the divergence is the Run history that this node itself rewrote by rebasing. The move goes through a two-parent `commit-tree` that keeps the **old tip as first parent**: no commit becomes unreachable, `git log` of the old tip stays intact, and the non-deletion rule of that decision record is unchanged. The guard is structural: the pipeline tip must still be exactly the `base_sha` from which this sub-worktree was cut, so it cannot adopt on top of another node's work.

Two side effects of the same defect are fixed along the way: `merge_conflict_detected.payload.detail` is finally filled in (it read `stderr`, which `git merge` leaves empty on conflict: the report is on stdout), and a merge-back that fails appends `NodeFailed` then reaps the node's session instead of leaving it alive under a `running` projection.

## 1.6.0

A breaking change shipped under a **minor** bump, in line with the previous ones set in 1.2.0 and 1.3.0: the daily surface is identical (the *Mark complete* button and `pdo complete` are in the same place and take the same arguments), and the removed behavior was a **lie**: eight exits that described a refusal answered `200`, four of them after having already killed the Run. No live configuration can depend on it without already being broken: a client that read "success" on these responses was wrong by construction. **If the maintainer prefers the letter of the preamble above ("breakage is announced here and by a major bump"), it is `2.0.0`: one word to change in `Cargo.toml`.**

### Breaking: a completion refusal is never a `200` again

The path through which **every** node ends, `pdo complete` (`POST .../nodes/<id>/done`) and *Mark complete* (`POST .../commands` `kind=mark_node_done`), answered `200` on eight bodies that described a refusal. An agent read `Node <id> marked complete.`, exited with `0`, and believed it had delivered a Run that the daemon had just killed. See the decision record on completion refusals.

The exits that go from `200` to **`409`**, with their new slug:

| Slug (`error`) | `recoverable` | Events already appended |
|---|---|---|
| `frontmatter_retry_pending` | `true` | `FrontmatterRetryPending` |
| `frontmatter_retry_exhausted` | `false` | `NodeFailed` + `RunFailed` |
| `script_validation_failed` | `false` | `NodeFailed` + `RunFailed` |
| `doc_violated_code_immutability` | `false` | `NodeFailed` + `RunFailed` |
| `merge_conflict` | `false` | `MergeConflictDetected` + `RunFailed` |
| `merge_resolution_failed` | `false` | `MergeResolverFailed` + `RunFailed` |
| `merge_resolver_spawned` / `merge_resolver_failed` | `false` | dead branches since an earlier decision record |

Two refusals **change body without changing status**: `completion_rejected` (the transition guard, already a `409`) sees its prose move from `error` to **`message`**, with `error` now carrying the slug; and `missing_outputs` simply gains `recoverable: true`.

- **Clients discriminate on `error`, never on the status.** A status does not have enough bits for nine causes, and the UI client was the demonstration: it re-read *every* `409` of this path as `missing_outputs` with an empty list, gated on `length > 0`, so the most frequent refusal of all, any click on a node of an already failed Run, displayed **nothing**. An unknown slug is rendered as is.
- **`recoverable` answers a single question**: *is it still your turn?* `true` means the node is still `running`, nothing terminal is recorded. `false` means the daemon has **already** recorded the terminal outcome; never follow up with `pdo fail`.
- **The detail is verbatim the same as before** (`missing`, `violations`, `detail`, `reason`): this batch moves a status and adds two keys, it renames no detail field. The `detail` of a `script` node's fail-fast stays **nested**: flattening it would make its audit trail indistinguishable from a failure after retry, and it is the **projector** that learns to read both forms.

### Breaking: `pdo complete` has an exit code contract

| Code | Meaning | Expected action |
|---|---|---|
| `0` | granted, or **legal duplicate** (`noop`) | nothing |
| `3` | refused, **still your turn** | fix, call `pdo complete` again. **Not** `pdo fail` |
| `4` | refused, **the runtime has already ruled** | stop and report. **Not** `pdo fail` |
| `1` | failure, transport, unreadable body, `5xx` | only here, `pdo fail` is the right advice |

**Why a `4` distinct from `1`.** The bash tail of a `script` node does `pdo complete || pdo fail`. That `||` was **dead code** as long as its trigger answered `200`; the switch to `409` wakes it up. Without discriminating the `4`, every terminal refusal of a `script` node would produce **two** `NodeFailed` and **two** `RunFailed`, the second with a wrong reason (`NodeFailed` is absorbed by the guard, `RunFailed` is not). The tail therefore tests for `4`, and a layer 3 test in real tmux + real bash counts exactly one `run_failed`.

**The `0` on a legal duplicate is non-negotiable**: a puzzled agent that calls `pdo complete` again must not read "refused" and then follow up with `pdo fail`: it would kill a Run that has just succeeded, and on a `script` node the tail would do it without asking.

### What does not change

- **The `2xx` semantics** (from an earlier decision record): "your terminal event is durably recorded and the advance is scheduled". It only gains a corollary: "and your completion was not refused".
- **The detached advance queue** and the **fail-fast** of a `script` node: intact.
- **`410`, `404`, `500`**: "never `2xx`" is **not** "always `409`". The tombstone of a forgotten Run keeps its `410`, an unknown target its `404`, a failure its `500`. On the `POST .../nodes/<id>/done` route, these three now carry the JSON body of the contract (`{error, recoverable, message}`) instead of plain text; their status is unchanged.
- **The success body of `POST .../done` stays the plain text `ok`**, asymmetric with the `{"ok":true}` of `POST .../commands`. Making them symmetric is planned separately.
- **The liveness watch**: unaffected by construction, since it reads the variant of the result, never the HTTP status.

### Migration

- **Direct callers** (`curl`, scripts, test harnesses): replace any test of the form `if resp.ok` or `if body.status == "..."` on this path with a read of `body.error` + `body.recoverable`. A `409` is no longer a transport anomaly, it is a **verdict**.
- **Hand-written bash of a `script` node**: if the body chains `pdo complete || pdo fail`, add the guard on exit code `4`, otherwise every terminal refusal produces a second `RunFailed` with a wrong reason. The tail generated by PDO already does it.
- **Nothing to do on the data side**: no event payload changes, `NodeState` gains a `missing_outputs` omitted when empty, and archived Runs project byte-identically, with one fix for a pre-existing bug: a node that turned green again no longer drags along the violations of the attempt it recovered from.

## 1.3.0

A breaking change shipped under a **minor** bump, in line with the previous one set in 1.2.0: the UI surface is identical (the "Target repository" field of New Run was already required on the client side, and its `repoValid` gate is tightened here), and no live configuration depends on the removed behavior: the 9 production Triggers all name their repository. **If the maintainer prefers the letter of the preamble above ("breakage is announced here and by a major bump"), it is `2.0.0`: one word to change in `Cargo.toml`.**

### Breaking: the daemon's cwd is no longer an implicit Run target

`target_repo` becomes a **required field** at the four write boundaries. A call without it, with an empty string or with whitespace answers **400 naming the field**, instead of silently creating its worktree in the repository from which the daemon was launched (`~/.pdo/app` in production). On 2026-07-29, two Runs had written code there, recoverable only through a `git fetch ~/.pdo/app`. See the decision record on the target repository.

- `POST /runs` (JSON **and** multipart): 400, before any effect: no `run_id`, no `run_started`, no worktree, no session;
- `POST /triggers`: 400, no row persisted;
- `PATCH /triggers/:id`: stays a **partial merge**. An **absent** `target_repo` leaves the stored value intact (this is what keeps the list toggle's `{"enabled": true}` legal); an explicit `null` or a blank string answers 400. Intended side effect: before, an explicit `null` was a **silent no-op** on the serde side, so clearing the repository field in the UI seemed to work and did nothing;
- `POST /triggers/guard/test`: 400. The dry-run executes an arbitrary `sh -c`; its "zero side effect" invariant was about not creating a Run, not about what the command does;
- a **Trigger whose target repository is null becomes dormant** instead of firing: the refusal comes up before the guard, so the guard is **never** launched (5 of the 9 live Triggers do `git pull` / `gh issue list`), `next_fire_at` goes to `NULL`, and "Run now" answers **409** with the reason instead of a `200 {fired:false}`;
- `retry_all` now copies the **resolved** repository and not the raw field. Without this, retrying a Run from before this change would return 400 **after** the original was archived: archived, with no replacement.

**What does not change, and never will**: the **read** fallback. An archived Run whose payload carries `target_repo: null` (about 46 of the 101 dev runs) stays readable, priceable and filed under the daemon's root repository, with no "Unassigned" bucket. The asymmetry is the design: mandatory where there is a caller to answer 400 to, resolved where there is only a past record to interpret.

**Migration.** No action required on an existing instance: the live Triggers all name their repository, and the UI already sent the field. **Direct callers** of the API (scripts, agents, `curl`, test harnesses) must name `target_repo`; the error message says what to pass. Archived Runs stay readable without intervention.

## 1.2.0

Two breaking changes shipped under a **minor** bump, by explicit decision of the maintainer: the v2.0.0 that the two slices had chosen was never tagged or published (the latest release is v1.0.0), and neither of the two breaks changes the surface that a user drives daily.

### Breaking: `session_died` is the only liveness verdict, the `Stale` state disappears

A node is no longer declared dead on an absence of activity: only the death of its session counts, and completion on an observed end of turn is opt-in (see the liveness and completion decision records). The `Stale` node state is removed: a client that expected it will no longer see it. New setting `autocomplete_turn_end` (`PDO_AUTOCOMPLETE_TURN_END`), off by default.

### Breaking: `GET`/`PUT /settings` lose `image_source` and `dockerfile_path`

The image source of a sandboxed Run is no longer an instance setting. It belongs to the **staging profile**, like the home content and the env (per the staging profile decision record): one axis per screen, the settings screen only answering "which profile does a Run take by default".

- `GET /settings` no longer contains `image_source`, `dockerfile_path`, or the `sandbox_image` disclosure (the tag that the resolved Dockerfile produced);
- `PUT /settings` carrying one of these fields answers **400 naming it**, instead of 200 silently ignoring it. A merely unknown field is still ignored, as before;
- the columns `instance_config.image_source` / `.dockerfile_path` become **inert**. They are not dropped, and a value lingering there triggers **one** `warn!` at boot that names it and points to the profile to edit. No Run is affected;
- `PDO_SANDBOX_IMAGE_SOURCE` and `PDO_SANDBOX_DOCKERFILE` are **kept** and repointed to the new profile default: a headless instance only has virtual profiles and no UI, so the env remains its only way to change image without POSTing a profile.

**Migration.** No action required if the two settings were never touched (their default value is now the constant `DEFAULT_PROFILE_IMAGE`: a hash-derived registry on the seeded Dockerfile, so a profile that sets no image produces exactly the same ref as before). If either carried a value, the boot says so: carry the choice over to the relevant profile via **Settings -> Manage staging profiles...**, or to the daemon's environment variable.
