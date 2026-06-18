# Component Library

_Started 2026-05-10 18:07 UTC_

---

## User



## Assistant

_[tool: list_files]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: snip]_

_[tool: write_file]_

_[tool: write_file]_

I've read the design system and constraints. Let me note out loud what I'm committing to so I stay consistent:

**System for this exploration**
- One HTML file with three vertically-stacked sheets (matrix → port rows → selection detail). No app chrome — sober dark sheet (`--bg-1`), Geist sans/mono.
- 5 status-outline hypotheses (H1-H5) and 5 port-row hypotheses (P1-P5), each scoped through CSS variables so a stroke width / halo offset / motion duration is one swap.
- No left ribbon anywhere. Type lives only on the icon channel. Status lives only on a full 4-side cadre (or its absence for `pending`). Selection lives on `--acc` outer ring **outside** the status cadre so they stack.

Let me build it.

_[tool: write_file]_

_[tool: write_file]_

_[tool: snip]_

_[tool: write_file]_

_[tool: show_html]_

_[tool: done]_

_[tool: snip]_

_[tool: fork_verifier_agent]_

Built `Node Component Exploration.html` — three sheets, all isolated components, no canvas chrome:

- **Sheet 1** — Status × treatment matrix. 5 hypotheses (Solid / Glow / Breathing pulse / Gap-offset ring / Marching dash) × 6 status rows including selected-on-running. Below the matrix, each hypothesis is replayed across Switch, Loop (with `break` on top and `done` on bottom), Merge, Start, and End (End showing the per-port indicator).
- **Sheet 2** — Port-row hypotheses. 5 hypotheses × 6 cells (right/left/top/bottom + valid-drop + debug-halo).
- **Sheet 3** — Selection cohabitation. 3 cells per hypothesis (running unselected → running selected → failed selected).

All treatments are themed through CSS variables (`--nc-cadre-w`, `--nc-halo-gap`, `--nc-halo-w`, `--nc-glow`, `nc-breath` / `nc-march` durations) so a stroke width or motion duration is a one-line swap. Selection always uses an outer `--acc` ring stacked **outside** the status cadre so both channels read at once.

## User

Verifier agent check completed

## Assistant

_[tool: str_replace_edit]_

_[tool: str_replace_edit]_

_[tool: str_replace_edit]_

_[tool: snip]_

_[tool: done]_

_[tool: fork_verifier_agent]_

_[tool: snip]_

Both verifier issues fixed:

- **Switch's three right-side ports** now distribute along the side via `idx`/`total` (same `t = 0.2 + i·0.6/(n−1)` spread used in `dag.jsx`).
- **Sheet 3 row headers** — the `.ph` flex-column rule now applies to both `.port-sheet .ph` and `.sel-sheet .ph`.

## User

do the same for these components

## Assistant

_[tool: snip]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: write_file]_

_[tool: grep]_

_[tool: grep]_

_[tool: grep]_

_[tool: grep]_

_[tool: snip]_

_[tool: write_file]_

_[tool: snip]_

_[tool: done]_

_[tool: snip]_

_[tool: fork_verifier_agent]_

Inspector component sheets are ready in **Inspector Component Exploration.html** — 4 port-row hypotheses (I1–I4), 3 output-card hypotheses (O1–O3), the type-driven field row, 3 Switch-branch hypotheses (S1–S3), and a cohesion check pairing one combo end-to-end. Pick a column from each sheet for the integration pass.

## User

Verifier agent check completed

## Assistant

_[tool: str_replace_edit]_

_[tool: snip]_

_[tool: str_replace_edit]_

_[tool: snip]_

_[tool: done]_

_[tool: snip]_

Fixed Sheet 1's overflow — the port-row hypotheses now lay out as a 3-up top row (resting / hover / focused) and a 2-up bottom row (multi-port list / side-mix), so everything fits within the doc width on any normal viewport.

## User

From inspector component I like :
* I1 Hairline
* 03 tab head
* F1
* S1 and rail
From node card and port row I like :
* Solid nodes
* Capsule pill

Can you update the existing mockup with those ? Adapt the inspector so it uses the amber accent though.

On top of adapting existing screens, create component spotlights for both of these explorations in the main file

## Assistant

_[tool: list_files]_

_[tool: grep]_

_[tool: read_file]_

_[tool: snip]_

_[tool: read_file]_

_[tool: snip]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: read_file]_

_[tool: snip]_

_[tool: read_file]_

_[tool: snip]_

_[tool: write_file]_

_[tool: snip]_

_[tool: str_replace_edit]_

_[tool: str_replace_edit]_

_[tool: str_replace_edit]_

_[tool: grep]_

_[tool: list_files]_

_[tool: grep]_

_[tool: read_file]_

_[tool: snip]_

_[tool: done]_

_[tool: fork_verifier_agent]_

_[tool: snip]_

Winners are wired into `PDO.html`:

- **`winners.css`** loaded after `styles-v2.css` — H1 status cadre on `.node`, P2 capsule pill on `.port-pill`, amber inspector scope, I1 hairline port rows, O3 tab-head outputs, F1 type-driven field rows, S1 AND-rail branches.
- **`dag.jsx`** now renders `<PortPill>` (capsule with directional chevron) instead of the old triangle handle. Legacy `.tri-handle` markup elsewhere (loop / switch / screens-v2) is hidden by `winners.css` so nothing leaks visually.

Pick this up next pass: port the same `PortPill` swap into `loop-node.jsx`, `switch-node.jsx`, `end-node.jsx`, and the inline ports in `screens-v2.jsx` so those special nodes get real pills instead of just empty edges.

## User

Verifier agent check completed

## Assistant

_[tool: read_file]_

_[tool: snip]_

_[tool: grep]_

_[tool: grep]_

_[tool: snip]_

_[tool: grep]_

_[tool: grep]_

_[tool: write_file]_

_[tool: str_replace_edit]_

_[tool: str_replace_edit]_

_[tool: snip]_

_[tool: done]_

_[tool: fork_verifier_agent]_

Added a new **Component spotlights — picked winners** section to `PDO.html` with three artboards:

- **W1 · Node card** — `.spot-node` matrix across all 5 statuses, selection cohabitation, and special-node family check (Switch, Loop, Start, End)
- **W2 · Port row** — `.port-pill` on all four sides, drop-target state, mixed-side Loop, and Switch fan-out
- **W3 · Inspector** — `.op-tab` output card (one expanded with `.fld` field rows incl. inline enum drawer, one collapsed) + `.sb-card` AND-rail Switch branches incl. dashed `.sb-default`, all under amber-scoped `.spot-inspector`

Loaded via new `spotlights.jsx` after `screens-v2.jsx`.

## User

running nodes should have a blue edge not amber

## Assistant

_[tool: str_replace_edit]_

_[tool: done]_

Fixed — `--st-running` is now hardcoded to `#3b82f6` blue and no longer inherits the accent. Running nodes will always show the blue cadre regardless of which accent swatch is selected.

