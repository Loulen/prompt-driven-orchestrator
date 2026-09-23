# `make readme-media`

Records the README media: the hero and one GIF (plus a JPG poster) per row of the features table.
Each **scene** is filmed on the real UI of a **demo instance** ([ADR-0074](../../docs/adr/0074-les-medias-du-readme-sont-joues-par-de-vrais-agents-sur-une-instance-de-demo-isolee.md),
CONTEXT.md « Médias du README »). The prerequisites, what a regeneration costs and when to run one
are in [CONTRIBUTING](../../CONTRIBUTING.md#readme-media).

```bash
make readme-media                 # every scene → .readme-media/ (not versioned)
make readme-media SCENE=stats     # one scene (comma-separated for several)
make readme-media SCENE=stats VARIANT=b   # one variant, while tuning a scene
make readme-media-publish         # selected variants → docs/assets/readme/<scene>.gif + .jpg
```

## What a run does

For each scene, in its own throwaway instance:

1. **Demo instance** (`lib/demo-instance.mjs`). It runs the checkout's `target/debug/pdo daemon` from a
   throwaway working directory (the event log follows the cwd), with a throwaway `HOME` (pipelines,
   skill bank, profiles, prices, transcripts) and a free port (the tmux socket `pdo-<port>` follows the
   port). Every `PDO_*` variable of the calling shell is dropped. A `pdo` pointing at the checkout's
   binary comes first on the node sessions' `PATH`. The library gets the demo pipeline
   `implement-review` (`fixture/pipelines/`), and `fixture/shop-app/` is copied as a fresh git repo.
   The demo `HOME` gets a `.tmux.conf` (focus events on, so Claude Code prints no tmux hint on
   camera), and the sessions find Playwright's browsers in your cache (`PLAYWRIGHT_BROWSERS_PATH`).
2. **Auth** (`lib/credentials.mjs`). Only for the harnesses the scene plays live (`live: ["claude"]`):
   the auth files are copied into the demo `HOME`. For claude that is `~/.claude/.credentials.json`,
   plus a `.claude.json` holding only the account block and the fixture repo's trust. Every file is
   recorded in `state.json` **before** it is written. The demo `~/.claude/settings.json` (not a
   secret) also sets Claude Code's status line to the model the session runs, so the filmed terminal
   reads `Opus 5.5 · claude-opus-5-5`.
3. **Mocked history** (`needs: ["history"]`, `lib/history-plan.mjs` + `lib/history-write.mjs`). About
   205 runs of `implement-review` over the last 30 days, with synthetic claude/copilot/pi transcripts,
   a manual price table and 100 fires of the (disabled) `prod-health-check` trigger. The plan is pure
   and deterministic (seeded); its targets are tested (`test/history-plan.test.mjs`) and so is the
   Stats API reading it (`test/demo-instance.test.mjs`).
4. **Recording** (`lib/recorder.mjs`). Playwright films each variant. The synthetic cursor
   (`lib/cursor.mjs`) is injected before the app on every document. Markers, kept windows and ×8
   stretches build the timeline. The page opens on a dark, ticking page, so the video starts at the
   timeline's 0 even when a scene waits before its first `goto`.
5. **Montage** (`lib/montage.mjs`). It keeps the timeline, cuts the waits, crops, and bakes in the
   window chrome (rounded corners, traffic lights, shadow). Then it encodes the GIF (15 fps, 960 px
   wide by default) and the poster, which is the last frame, i.e. the end state. ffmpeg runs
   asynchronously, so a Ctrl+C mid-encode is handled at once and kills it. Each variant is encoded
   under `.work/` and lands in the review folder together with its manifest entry: an interrupted
   run leaves the previous GIF and its entry, never a new GIF under a stale entry.
6. **Teardown**. The daemon is stopped, the demo tmux server is killed (every agent with it), and
   the script **waits for every process of the demo to really exit** before it goes on: the panes
   and their descendants, plus (Linux) any process whose HOME is the demo HOME or whose cwd is under
   the root. A `claude` that got the hangup takes a few seconds to wind down and writes under its
   HOME on the way out, so removing the root before it exits would let it recreate an orphan
   `/tmp/pdo-readme-media-*`. A process still alive after 10 s is killed. Then the auth files are
   wiped and the root is removed. This happens after each scene, and on any exit:
   failure, uncaught error, Ctrl+C, SIGTERM. A run killed with SIGKILL leaves its `state.json`, and
   the next start finishes the teardown. `KEEP_DEMO=1` keeps the root for inspection (auth still wiped).

Output: `.readme-media/<scene>/<variant>.gif|.jpg`, `.readme-media/manifest.json` (per variant:
duration, weight, dimensions, markers, warnings), and the raw takes with their cut plan
(`timeline.json`) under `.readme-media/.work/`.

## Choosing and publishing

`selection.txt` (versioned) holds one line per README scene: `<scene> <variant>`.
`make readme-media-publish` copies **only** those variants from `.readme-media/` into
`docs/assets/readme/<scene>.gif` and `.jpg`, the paths `README.md` and `docs/readme/README.fr.md`
point at. To publish the other variant, edit the line and publish again: nothing is recorded. A
line whose scene has no file yet is skipped. A selected variant that was never recorded is an
error.

## Adding a scene

A scene is **one file**: `scenes/<name>.mjs`. The folder is listed at run time, so there is no
registry to edit, and two scenes written in parallel never touch the same file. `selection.txt`
already has a line for every scene of the README. Files starting with `_` are helpers, not scenes.

```js
// scenes/triggers.mjs
import { sleep } from "../lib/demo-instance.mjs";

export default {
  name: "triggers",            // = the file name, = the README slot docs/assets/readme/triggers.gif
  title: "Triggers",
  needs: ["history"],          // seed the mocked history (runs, Stats, trigger fires)
  live: [],                    // harnesses played live: their auth is staged, e.g. ["claude"]
  async setup(instance) {},    // optional: API calls on the fresh instance (instance.api(...))
  variants: [                  // exactly two
    {
      id: "a",
      label: "Guard dry-run, then fire history",
      viewport: { width: 1200, height: 760 },
      crop: { x: 180, y: 60, width: 1020, height: 640 },   // optional: focused crop (zoom)
      gifWidth: 960,                                         // optional (the hero may be wider)
      localStorage: { "pdo.layout.run": { left: 15, center: 42, right: 43 } }, // optional
      markers: ["guard-tested", "history-shown"],            // the manifest checks they were set
      async play(ctx) {
        await ctx.goto("/");
        await ctx.click(ctx.page.getByText("Triggers"));
        ctx.mark("guard-tested");
        await ctx.hold(1500);                                // end state = poster
      },
    },
    { id: "b", /* … */ },
  ],
};
```

What `play(ctx)` gets:

| | |
| --- | --- |
| `ctx.page`, `ctx.instance` | the Playwright page, and the demo instance (`url`, `repo`, `home`, `api(method, route, body)`) |
| `ctx.goto(route)` | open a route of the demo UI; the cursor stays where it was |
| `ctx.moveTo(target)`, `ctx.click(target)`, `ctx.hover(target)`, `ctx.drag(from, to)`, `ctx.scroll(dy)` | eased, real mouse gestures. `target` is a locator or `{x, y}`. The cursor shows a green ring on click, is pressed (×0.88) during a drag, and draws a dashed orange line |
| `ctx.mark(name, { before, after })` | a key moment. The GIF keeps `before`/`after` ms around it (default 1.2 s / 1.8 s) |
| `await ctx.keep(fn)` | keep what `fn` films at normal speed (a gesture between markers) |
| `await ctx.fast(fn, { speed: 8 })` | film `fn` fast-forwarded (an agent working) |
| `await ctx.hold(ms)` | stand still and keep it. End every variant on one: the last frame is the poster |

Whatever is not in a marker window, a `keep`, a `fast` or a `hold` is cut. A gesture made with
`ctx.page` directly (a `locator.click()`, `page.mouse`) is fine off camera, but the cursor overlay
follows it: end it where the next filmed gesture starts. Aim for 8 to 15 s; the
manifest warns outside that range, and a short montage is frozen on its last frame up to 8 s.
Nothing on screen may change after the final `hold`: the video runs a beat past it, and that beat
can become the poster.
A row GIF shows a ~480 px cell: crop on the panel that matters so its text reads at about 1:1
(crop width close to the GIF's content width, ~910 px); a narrower `viewport` lets the page reflow
into that crop. Record the scene, open both GIFs from `.readme-media/`, pick one in `selection.txt`, then
`make readme-media-publish`.

## The scenes so far

| Scene | Live | What it films | Helpers |
| --- | --- | --- | --- |
| `hero` | `claude` | a real run of `implement-review`: a → zoom on `implementer` and its live terminal; b (published, design « Variant B ») → live terminal ×8, cut, the `reviewer`'s outputs (verdict + `image_list`) at 1280×760 | `scenes/_live.mjs` |
| `pipelines` | — | from the pipeline's skeleton (Start, End): + → Node (`implementer`), then its edges | `scenes/_canvas.mjs` |
| `routing` | — | the loop edge `reviewer → implementer`, `verdict != pass`, the exit as `else`, saved | `scenes/_canvas.mjs` |
| `stats` | — | the Stats page over the mocked history | |
| `outputs` | `claude` | a finished run's `reviewer`: its two ports in Edit, then in Run an annotated screenshot (lightbox) and the `review` markdown with its Mermaid diagram rendered | `scenes/_live.mjs` |
| `review` | `claude` | a finished run's Review page: a comment on a line, sent to the manager, cut ×8, the manager's own answer in the thread | `scenes/_live.mjs` |
| `orchestration` | `claude` | `implementer` with Orchestrator on relaunches `implement-review` once per part: the children nest under their parent in the run tree, the counters follow them to the end | `scenes/_live.mjs`, `scenes/_canvas.mjs` |
| `triggers`, `profiles`, `skills` | — | settings, no agent (see below) | `scenes/_no-agent.mjs` |

`scenes/_live.mjs` is for the scenes that need real agents: `startDemoRun`
starts a run of the demo pipeline on the fixture repo (`DEMO_TASK`: add a product search),
`completeDemoRun` plays one to the end off camera (a scene's `setup`: `outputs` films one finished run
for both variants, `review` plays one per variant, side by side), `waitNode` / `waitRun` / `waitPane` wait for a node's or a
run's status or for text in its tmux pane, and `stopDemoRun` stops every agent of the run, in a
`finally`, as soon as the variant is filmed: its nodes, its manager (`pdo-mgr-<run>`, started on
demand by a send to the manager), and with `children: true` every child run first (`archive: true`
also takes them off the runs rail). `scrollUntil` wheel-scrolls a panel until an element sits at a
given height: point it `over` a visible element of the panel, never the (off-screen) target. `openRun` selects a run on the rail off camera. Opening a run selects
its live node by itself, so select `Start` first if the filmed click must open the node.
`scenes/_canvas.mjs` is for the edit canvas: `installPipeline` / `restoreDemoPipeline` put a variant's
starting pipeline in place (a variant that saves changes the library for the next one), `handle`,
`pointOnEdge` (a point on an edge's drawn route: its hit box's centre is not on it) and `zoomCanvas`.

## Artifact scenes (#859)

- **outputs**: `setup` plays a whole run (`completeDemoRun`); both variants open its `reviewer`.
  The reviewer's prompt (`fixture/pipelines/implement-review.prompts/reviewer.md`) keeps the review
  short, with a small left-to-right Mermaid diagram (three or four boxes) right after the verdict:
  it reads in the modal without a scroll (a top-down one scales to the modal's width and overflows). The wait for the Mermaid render is cut; the thumbnails and the lightbox image are loaded
  before they are filmed. The poster checks the run tab has nothing unsaved. On the Run tab the
  finished node's terminal is folded to a bar (#346) and the panel lists its I/O: the one input
  (`code`) above the two outputs. That is how the app shows a finished node, and the scene films it
  as is.
- **review**: `setup` plays **one whole run per variant**, both at once: a variant never films the
  other's comment or reply ("1 sent", a thread already answered). Each variant comments a line the
  demo task always adds (`app.js`'s input listener, `index.html`'s search input; the file's first
  added line otherwise), sends it, and waits for the reply the manager posts with
  `pdo review reply` **in that comment's thread**: nothing is injected. The wait is a short ×8
  stretch, then a cut. The manager is stopped after each variant. Both variants film the unified
  view with the file list closed, at 1040 px (barely scaled to the GIF's 960, nothing cropped): no code line,
  toolbar button or thread footer is cut or wrapped.
- **orchestration**: `setup` turns Orchestrator on for `implementer` in the demo HOME's copy of
  `implement-review` (same name: still the one pipeline). The run's task spells out the two
  `pdo run create implement-review …` commands, each child's task says not to orchestrate in turn.
  The children are real runs of the whole pipeline (a minute or two each); the variant ends when
  both are finished, then stops and archives the parent and its children. Before variant a's
  poster, `implementer` is re-selected off camera once its session ended, so its terminal folds to a
  bar instead of an empty « [exited] » block.

## Settings scenes (no live agent)

`triggers`, `profiles` and `skills` (#857) film settings, not agents: `live: []`, so no auth is
staged. `scenes/_no-agent.mjs` makes that hold. In the page, every request that could start an agent
session (a new run, a trigger's Run now, a retry) is aborted, and after each variant the demo tmux
socket must hold no session.

- **triggers**: the demo trigger (`* * * * *`, guard `./prod-health-check.sh`) is shown **armed**.
  The global Trigger pause goes on first, then the trigger is enabled, so the scheduler never fires
  it. The crop leaves out the left panel and its pause banner. The guard is a script of the fixture
  repo (`fixture/shop-app/prod-health-check.sh`, probe in `ops/prod-probe.env`): it exits 0 and prints
  the same incident report the mocked history's fired entries carry.
- **profiles**: the scene rewrites the demo HOME's copy of `implement-review` so both nodes follow
  one profile, « daily driver » (claude · opus · medium). Each variant resets that profile before it
  plays.
- **skills**: the source is a local git repo, `fixture/qa-skills/` copied to `~/code/qa-skills` in
  the demo HOME, so nothing goes over the network. Each variant resets the bank to what the fresh
  instance seeded. The pick on `reviewer` is never saved.

## Tests

`make test` runs `node --test scripts/readme-media/test/*.test.mjs`. That covers the history plan,
the cut plan, selection and publication, scene discovery (and the markers of each scene), the
manifest (and a variant's atomic landing), a SIGINT mid-encode, the video/timeline alignment, and a
real demo instance
(Stats API ratios, teardown after success / Ctrl+C / crash / hard kill, and an agent that outlives
the hangup; needs `cargo build`), plus
the settings scenes' declarations and fixtures (the guard's exit codes and report, the skills repo).
The full recordings (Stats, and triggers + profiles + skills) are opt-in: `READMEMEDIA_E2E=1`.
