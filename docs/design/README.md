# Maestro — Design Bundle

This folder is the **source of truth for visual design** of Maestro. It's a Claude Design ([claude.ai/design](https://claude.ai/design)) handoff bundle, exported and committed for durability.

It carries **two generations** of screens:

- **v1 carry-overs** (`screens.jsx` + `styles.css`) — the screens that survived the 2026-05-09 design iteration: empty state, markdown viewers, Switch/Loop inspectors, library dropdown, Loop+Switch run, star save-state strip.
- **v2 — unified-mode + inline terminal + ForEach/Merge** (`screens-v2.jsx` + `styles-v2.css`) — the 12 new screens produced from [`design-prompts/maestro-unified-mode-inline-terminal.md`](../../design-prompts/maestro-unified-mode-inline-terminal.md). They obsolete v1's run/edit-mode dichotomy (toggle removed), the polled tmux preview (replaced by inline xterm.js), the auto-spawned merge resolver (replaced by first-class `Merge`), and add `ForEach`, typed Outputs schema, lint info-only diagnostic, and pipeline-info panel with Manager terminal.

`Maestro.html` loads both `screens.jsx` and `screens-v2.jsx`. The design canvas renders all surviving v1 screens + all new v2 screens side-by-side as artboards.

---

## How to read this folder

- **`project/Maestro.html`** — entry point. Renders all artboards on a design canvas. Open in a browser only if you genuinely need to (the source is more informative).
- **`project/styles.css`** — v1 design tokens (colors, typography, geometry) + v1 component styles. Read first when implementing any UI piece.
- **`project/styles-v2.css`** — v2 deltas: classes specific to the new screens (xterm wrapper, info panel, ForEach/Merge nodes, schema editor rows, lint banner, etc.). Read in conjunction with `styles.css` — they layer.
- **`project/screens.jsx`** — composition of the surviving v1 screens (`Screen8`, `9`, `9b`, `13`, `14`, `15`, `16`, `17`).
- **`project/screens-v2.jsx`** — composition of the 12 v2 screens (`ScreenA1`–`A4`, `ScreenB5`–`B8`, `ScreenC9`, `ScreenS10`–`S12`).
- **`project/*.jsx`** — modular components (chrome, dag, runs-list, inspector, modal, md-modal, start-node, end-node, icons, switch-node, loop-node, library-dropdown, confirm-modal, design-canvas, tweaks-panel). The HTML prototype uses Babel-standalone in-browser; production target is React + Vite + xyflow + shadcn/ui (cf. ADR-0003).
- **`project/data.js`** — sample data shape. Hint for the daemon's HTTP API contract.
- **`chats/chat1.md`** — initial design conversation (2026-05-06, 8 v1 screens).
- **`chats/chat2.md`** — v2 iteration conversation (2026-05-09, screen cleanup + 12 new screens).
- **`HANDOFF-README.md`** — auto-generated coding-agent README from Claude Design (kept verbatim for reference).

---

## What's covered

### v2 screens — 2026-05 iteration (12 screens, in `screens-v2.jsx`)

Driven by [`design-prompts/maestro-unified-mode-inline-terminal.md`](../../design-prompts/maestro-unified-mode-inline-terminal.md), itself a synthesis of ADR-0005, 0006, 0007.

**Happy path A — Monitor and intervene during a Run** (4 screens)

| # | Screen | Source | Reflects |
|---|---|---|---|
| A1 | Run — pipeline running, node terminal at default height | `ScreenA1` | ADR-0005 + § *Sessions tmux / Pont UI ↔ tmux* |
| A2 | Run — pipeline-info panel open with Manager terminal dominant | `ScreenA2` | § *UX / Toolbar info pipeline* + § *Pipeline Manager* |
| A3 | Run — edit during run: node added mid-run, mutation invariants | `ScreenA3` | ADR-0007 + § *Édition pendant un Run* |
| A4 | Run — failed node with frontmatter retry exhausted | `ScreenA4` | § *Frontmatter / Validation à la complétion + fallback tmux* |

**Happy path B — Design a pipeline from the library** (4 screens)

| # | Screen | Source | Reflects |
|---|---|---|---|
| B5 | Library — pipeline template selected, canvas in design context | `ScreenB5` | § *Bibliothèque* + § *UX unifié* |
| B6 | Node inspector — output schema editor with enum | `ScreenB6` | § *Frontmatter / Schéma déclaratif par output port* |
| B7 | Canvas — ForEach + Merge convergence | `ScreenB7` | § *ForEach* + § *Merge* + ADR-0006 |
| B8 | Canvas — fan-out without Merge: lint info-only diagnostic | `ScreenB8` | ADR-0001 (sharp tool, lint info-only) |

**Happy path C — Launch a new Run** (1 screen)

| # | Screen | Source | Reflects |
|---|---|---|---|
| C9 | + New Run modal — pipeline picker fed from starred templates | `ScreenC9` | § *Bibliothèque* + § *Workflow utilisateur typique* |

**Component spotlight** (3 screens)

| # | Screen | Source | Reflects |
|---|---|---|---|
| S10 | Spotlight — inline xterm.js terminal expanded | `ScreenS10` | ADR-0005 / expanded variant |
| S11 | Spotlight — pipeline info panel with star popover (diverged) | `ScreenS11` | § *Bibliothèque* / favoriter pipeline |
| S12 | Spotlight — output schema row, enum-with-allowed editor | `ScreenS12` | § *Frontmatter / Schéma* / enum editor zoom |

### v1 carry-overs (8 screens, in `screens.jsx`)

| # | Screen | Source | Reflects |
|---|---|---|---|
| 08 | Empty state — first launch | `Screen8` | § *UX / Status icon par Run* |
| 09 | Markdown viewer modal — verdict (single file) | `Screen9` | Right-panel I/O ports → port content viewer |
| 09b | Markdown viewer modal — repeated port w/ navigator | `Screen9b` | Right-panel I/O for `repeated: true` ports |
| 13 | Switch inspector — branch list with predicate editor | `Screen13` | § *Switch* + ADR-0002 |
| 14 | Loop inspector — counter + body subgraph hint | `Screen14` | § *Loop* |
| 15 | Library dropdown (legacy) | `Screen15` | superseded by v2 fused left panel — kept for reference |
| 16 | Loop + Switch run — review-loop in flight | `Screen16` | § *Loop* + § *Switch* runtime composition |
| 17 | Star save-state strip | `Screen17` | Library star semantics (synced / diverged / orphan) |

### Removed v1 screens (deleted in 2026-05-09 cleanup)

`Screen1`, `Screen2`, `Screen3`, `Screen4`, `Screen6`, `Screen7`, `Screen10`, `Screen12` — all relied on the now-removed Edit/Run toggle, polled tmux preview, or have explicit v2 replacements. See `chats/chat2.md` for the rationale.

---

## v2 helpers introduced in this iteration

In `screens-v2.jsx`:

- **`XTerm`** — the inline xterm.js terminal placeholder with `expand` / `detach` controls and an `awaiting`/`focused`/`scrolled` state matrix. Used in `NodeDetailV2` and `PipelineInfoPanel`.
- **`UnifiedLeftPanel`** — replaces `RunsListPanel` + `PipelinesListPanel`. One panel hosting Runs (filterable) + Library (Pipeline templates with stars + Reusable nodes draggable).
- **`RunOverlayV2`** — Run overlay with the *Edit this run* button removed (no more local toggle) and an optional `linkedTemplate` chip.
- **`CanvasToolbar`** — the canvas toolbar gains an `i` info icon next to the placeable node types. Click toggles `infoOpen`.
- **`PipelineInfoPanel`** — pipeline-level info panel reachable via the toolbar `i`. Two variants: idle (metadata only) and running (Manager terminal dominant). Hosts the favorite/star action with a `popoverOpen` for the diverged-state actions.
- **`NodeDetailV2`** — replaces `NodeDetail`. Inline xterm.js instead of polled preview. Supports `expanded`, `retry` (frontmatter validation pending), `failedValidation` (retry exhausted) states.
- **`NodeInspectorEditV2`** + **`OutputSchemaEditor`** — outputs section becomes an editable typed schema (`enum`, `int`, `string`, `bool`, `list`). Inputs remain untyped (asymmetry intentional, cf. `CONTEXT.md § Frontmatter`).
- **`ForEachNode`** + **`IcForEach`** + **`MergeNode`** + **`IcMerge`** — first-class node types and their toolbar icons. ForEach has 2 in / 2 out (`in`, `break`, `body`, `done`) with parallel iteration semantics; Merge has 1 input `branches: repeated` and 1 output `merged`.
- **`LintBanner`** — info-only diagnostic that surfaces near a fan-out lacking a downstream Merge.
- **`NewRunModalV2`** — uses the existing `.modal-bg` / `.modal` chrome; picker is fed from starred templates only.

In `icons.jsx`, six new glyphs were added for the v2 surfaces: `Bookmark`, `Cursor`, `ArrowRight`, `Lock`, `Minimize`, `Play`.

---

## Design tokens — quick reference

From `project/styles.css` (v1 base, unchanged):

### Surfaces (cool dark palette)

```
--bg-0  #0a0c10  app outer
--bg-1  #0f1115  canvas / center panel
--bg-2  #14171d  side panels
--bg-3  #1a1e25  hover, raised cards
--bg-4  #232831  node bodies, modal
--bg-5  #2c323d  ridge, divider strong
```

### Text

```
--fg    #e6e8eb  primary
--fg-2  #aab1bd  secondary
--fg-3  #767e8c  muted
--fg-4  #525a68  disabled
--fg-5  #383e48  ghost
```

### Status colors (used everywhere — Run dots, node border-left, badges, etc.)

```
--st-running  #3b82f6  blue (pulses)
--st-await    #f59e0b  amber
--st-done     #10b981  emerald
--st-blocked  #f97316  orange
--st-failed   #ef4444  red
--st-archived #6b7280  gray
--st-pending  #4b5563  darker gray
```

### Accent

```
--acc        #10b981  emerald (default; tweakable to violet/blue/amber)
```

> **`--edit-tint` is deprecated** since the v2 iteration. The variable still exists in `styles.css` (carry-overs may reference it) but no v2 screen uses it. Do not introduce new uses.

### Typography

```
--font-sans  Geist
--font-mono  Geist Mono
```

### Geometry

- Top bar: 44 px
- Status bar: 22 px
- Left panel: 280 px (resizable)
- Right panel: 360 px (resizable)
- Border-radius default: 6 px (small: 4 px, large: 10 px)
- Body font size: 13 px / line-height 1.45
- Letter-spacing: -0.005em on sans, default on mono

`styles-v2.css` adds classes for the v2-specific UI (xterm wrapper, info panel layout, ForEach/Merge node visuals, schema editor rows, lint banner, etc.) on top of the v1 base.

---

## Implementation notes (Rust + React port)

When porting to the production stack:

- **Keep the CSS tokens as-is.** Copy `styles.css` `:root` + `styles-v2.css` deltas into the React app's global CSS.
- **Port components 1-to-1 to React + shadcn primitives.** Use Vite + TS. Don't carry the Babel-standalone scaffolding.
- **DAG canvas (`dag.jsx` + node/edge components)** → re-implement on top of **xyflow**. The new `ForEachNode` and `MergeNode` map cleanly to xyflow's custom node API; their port shapes (`branches: repeated` for Merge, `body`/`done` for ForEach) are part of the daemon's pipeline schema, not just visual sugar.
- **Inline terminal (`XTerm` placeholder in `screens-v2.jsx`)** → real xterm.js (`@xterm/xterm` + addons `fit`, `web-links`) wired to the daemon's `WS /sessions/<id>/pty` route (cf. ADR-0005). The placeholder mock in the design is just an indicative box — the real implementation gets keyboard input, copy/paste, scrollback, and a configurable `expand` toggle.
- **Inspector forms (`inspector.jsx` + `NodeInspectorEditV2`)** → use shadcn `Input`, `Select`, `Textarea`, `Switch`, `RadioGroup`. The new typed-output rows (`OutputSchemaEditor`) need a `Select` for the type and a chip-list for `enum`'s `allowed:`.
- **Modal (`modal.jsx`)** → shadcn `Dialog`. `NewRunModalV2` reuses the same shell.
- **Markdown viewer modal (`md-modal.jsx`)** → shadcn `Dialog` (~640 px wide, max 80vh) + react-markdown + remark-gfm. Repeated-port navigator is a small prev/next pair.
- **Start / End nodes** → custom xyflow node types. Mandatory authored YAML nodes (`type: start`, `type: end`); pipeline parser fails if either is missing. Render in any context, draggable but cannot be deleted, no tmux sessions, no prompt files. Start has one output `user_prompt` (filled from `_input.md`); End has one input `result`.
- **ForEach / Merge nodes** → custom xyflow node types. ForEach: 2 inputs (`in`, `break`), 2 outputs (`body`, `done`). Merge: 1 input `branches: repeated`, 1 output `merged`. Always `code-mutating`. Rendered with distinct visual signature (cf. `ForEachNode`, `MergeNode` in `screens-v2.jsx`).
- **Triangle handles** → all node ports render as triangles, not circles. Direction encodes flow: input = inward, output = outward. Each port has a `side` field (`left` | `right` | `top` | `bottom`) that picks the rotation; defaults are `left` for inputs, `right` for outputs. Authors can override per port for nice cycles.
- **Stable id + name** → every node carries a `id` (immutable nanoid, system-generated) and a `name` (user-visible, renameable). Edges, prompts, and on-disk paths key by `id`.
- **Run overlay (`RunOverlayV2`)** is custom (floating card with backdrop-blur) — not a shadcn primitive.
- **Pipeline info panel + Manager xterm** → reachable via toolbar `i`. Same `XTerm` component, attached to the `maestro-mgr-<run-id>` tmux session via `WS /sessions/<id>/pty`. Idle pipelines render the panel without the terminal slot.
- **Sample data shape (`data.js`)** is a hint for the **HTTP endpoint contract**: `GET /runs/<id>` returns roughly `RUNS[i]`; pipeline-graph endpoint returns `FWR_NODES` + `FWR_EDGES` shape.

---

## Known divergences / things to validate

- **Node type naming**: design uses short labels `code` / `doc` on badges, but the segmented control in the node inspector shows the canonical `code-mutating` / `doc-only`. Spec uses canonical names in YAML. Keep both: visual badges abbreviated, YAML/code uses full names.
- **Tweaks panel default** in `Maestro.html` is `accent: #f59e0b` (amber); `styles.css` `:root` has `--acc: #10b981` (emerald). The CSS root is the real default — Tweaks is a play-around panel for the design tool. Use emerald as the production default.
- **Pipeline example** in design (`feature-with-review`: Plan → Implementer ↔ Reviewer + Tests + Merge) is richer than the brief's example. It's sample data, not contract. The spec doesn't pre-can pipelines (cf. ADR-0001).
- **Daemon-disconnected banner** is defined in CSS (`.dc-banner`) but not present in any v2 screen. Add as a screen variant if needed during implementation.
- **Info panel surface placement** (in place of the right panel vs. as an overlay) was left to design's discretion in the brief. The current `PipelineInfoPanel` renders in place of the right panel; if at implementation time we prefer an overlay, the component is structured to allow either.
- **`chrome.jsx > TopBar` still renders the pencil Edit toggle** (with `mode` / `onToggleMode` props) — Claude Design did not strip it, only stopped invoking `<Frame>` / `<TopBar>` from the v2 screens. The pencil therefore still appears on the v1 carry-over artboards (Screen8 etc.). At React port time, remove the pencil button + `mode`/`onToggleMode` props from `TopBar`; the production app has no edit mode.
