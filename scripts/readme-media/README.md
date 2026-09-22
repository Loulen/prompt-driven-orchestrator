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
2. **Auth** (`lib/credentials.mjs`). Only for the harnesses the scene plays live (`live: ["claude"]`):
   the auth files are copied into the demo `HOME`. For claude that is `~/.claude/.credentials.json`,
   plus a `.claude.json` holding only the account block and the fixture repo's trust. Every file is
   recorded in `state.json` **before** it is written.
3. **Mocked history** (`needs: ["history"]`, `lib/history-plan.mjs` + `lib/history-write.mjs`). About
   About 205 runs of `implement-review` over the last 30 days, with synthetic claude/copilot/pi transcripts,
   a manual price table and 100 fires of the (disabled) `prod-health-check` trigger. The plan is pure
   and deterministic (seeded); its targets are tested (`test/history-plan.test.mjs`) and so is the
   Stats API reading it (`test/demo-instance.test.mjs`).
4. **Recording** (`lib/recorder.mjs`). Playwright films each variant. The synthetic cursor
   (`lib/cursor.mjs`) is injected before the app on every document. Markers, kept windows and ×8
   stretches build the timeline.
5. **Montage** (`lib/montage.mjs`). It keeps the timeline, cuts the waits, crops, and bakes in the
   window chrome (rounded corners, traffic lights, shadow). Then it encodes the GIF (15 fps, 960 px
   wide by default) and the poster, which is the last frame, i.e. the end state. ffmpeg runs
   asynchronously, so a Ctrl+C mid-encode is handled at once and kills it. Each variant is encoded
   under `.work/` and lands in the review folder together with its manifest entry: an interrupted
   run leaves the previous GIF and its entry, never a new GIF under a stale entry.
6. **Teardown**. The daemon is stopped, the demo tmux server is killed (every agent with it), the
   auth files are wiped and the root is removed. This happens after each scene, and on any exit:
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

Whatever is not in a marker window, a `keep`, a `fast` or a `hold` is cut. Aim for 8 to 15 s; the
manifest warns outside that range, and a short montage is frozen on its last frame up to 8 s.
A row GIF shows a ~480 px cell: crop on the panel that matters so its text reads at about 1:1
(crop width close to the GIF's content width, ~910 px); a narrower `viewport` lets the page reflow
into that crop. Record the scene, open both GIFs from `.readme-media/`, pick one in `selection.txt`, then
`make readme-media-publish`.

## Tests

`make test` runs `node --test scripts/readme-media/test/*.test.mjs`. That covers the history plan,
the cut plan, selection and publication, scene discovery, the manifest (and a variant's atomic
landing), a SIGINT mid-encode, and a real demo instance
(Stats API ratios, teardown after success / Ctrl+C / crash / hard kill; needs `cargo build`). The
full recording of the Stats scene is opt-in: `READMEMEDIA_E2E=1`.
