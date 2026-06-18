# PDO — Visual Editor & Run Dashboard Design Brief

> Self-contained design brief for a visual generation tool (Figma Make / v0 / Lovable / Bolt / etc.). Hand this whole document to the tool as the prompt. No external context required.

---

## 1. What PDO Is

PDO is a desktop-style web application (runs as a local web app in v1, will move to a Tauri desktop app later) that lets a software engineer **design and execute deterministic AI-agent pipelines** to produce code. The user is a senior engineer working alone on their own laptop. PDO is *not* a SaaS, *not* multi-user, *not* mobile.

Mental model: think "Apache Airflow for AI coding agents" but with a single-user offline-first focus, and where **each node in the DAG is an autonomous Claude Code session running in its own tmux session inside a dedicated git worktree**. The pipeline DAG is fixed at design time (no LLM-driven routing); only the leaf agents are stochastic. Conditional edges are allowed but evaluate only deterministic predicates (iteration counters, frontmatter fields, pipeline variables) — never LLM judgment.

**Core value proposition for the user**: PDO is an **atelier of code production**, not a fire-and-forget orchestrator. The user spends real time crafting pipelines and inspecting runs. The UI must reward attention, not fight it.

---

## 2. Stack & Constraints

- **Framework**: React + Vite + TypeScript
- **DAG canvas**: [xyflow](https://reactflow.dev) (formerly React Flow) — use its primitives for nodes, edges, handles, mini-map, controls
- **Component library**: shadcn/ui (Radix + Tailwind) — use its dialogs, dropdowns, forms, badges, tooltips, sheets, command palette
- **Styling**: Tailwind CSS, dark mode by default with light mode toggle
- **Realtime**: WebSocket (state pushed from the local Rust daemon to the React app)
- **Single window** (v1, web app in browser; v2 will wrap the same UI in Tauri)

The visual style should feel like a **modern developer tool** in the spirit of Linear, Raycast, Vercel dashboard, or VS Code. Dense information, generous spacing inside panels, sharp typography (Inter or Geist), monospace where data is shown (paths, ids, code, terminal previews). Subtle animations only — fades and small slides, no flashy transitions.

---

## 3. Two Top-Level Modes

The app has exactly two modes, toggled by a **pencil icon** in the top-right of the app chrome:

1. **Run mode** (default at launch). The user monitors active and recent pipeline runs, launches new runs, inspects what's happening, talks to the manager agent of a run.
2. **Edit mode**. The user creates and edits pipeline definitions on a DAG canvas.

Both modes share the same **3-panel layout** (left list / center main / right detail), but their contents differ. The pencil icon toggles. The toggle should be obvious — when in Edit mode, give the entire chrome a subtle "you are editing" cue (a thin colored top border, or a tinted left panel header).

---

## 4. App Chrome (Always Visible)

A top bar, ~48px tall:

- **Left**: app logo + name "PDO" (small).
- **Center**: breadcrumb showing current context (e.g. "Run / `feature-with-review` / Run #2026-05-06-1430-a3f", or "Edit / `feature-with-review.yaml`").
- **Right**: pencil icon (mode toggle), settings gear, dark/light theme toggle.

A bottom status bar, ~24px tall (optional but nice):

- Daemon connection status (green dot if connected, amber if reconnecting).
- Active runs count (e.g. "3 runs active, 2 awaiting user").

---

## 5. Run Mode — 3-Panel Layout

### 5.1 Left Panel — Runs list (~280px wide, resizable)

Header:
- Title "Runs"
- Filter dropdown (All / Active / Done / Failed / Archived)
- "+ New Run" button (primary, prominent — kicks off the launch modal, see §7.1)

List body:
- Each item is a card or row representing one Run.
- **Status icon** at the left of each row, color-coded:
  - Blue with subtle pulse: `running`
  - Yellow: `awaiting_user` (an interactive node is waiting for the user)
  - Green: `done`
  - Orange: `blocked` (run halted by a `when:` clause or merge conflict unresolved)
  - Red: `failed`
  - Gray: `archived` (cleaned up, history preserved)
- Item content: pipeline name (e.g. "feature-with-review"), small subtitle showing the run's input head ("Implement search filter…"), timestamp (relative: "3min ago", "2h ago").
- Active runs sorted to top, then by recency.
- Click on a row → selects the run, populates the center and right panels.
- Right-click / kebab menu on each row: "Cleanup" (which is really Archive — keeps event history, removes branch/worktree/artifacts), "Open Manager" (see §6.1), "Copy run-id".

### 5.2 Center Panel — Pipeline graph for selected Run

The bulk of the screen. Renders the pipeline DAG of the currently selected run using xyflow.

- **Nodes** are styled as rounded rectangular boxes (~180×80 px), with:
  - Node name (top, bold).
  - Node type badge: `code-mutating` (small icon: pen-on-code) or `doc-only` (small icon: document).
  - Status indicator border or fill — same color scheme as the run-status icons (blue pulsing if running, gray if pending, green if done, etc.).
  - For nodes inside a topological cycle (with a back-edge), show an iteration counter `iter: 3/5` if applicable.
  - Input/output handles on left/right sides, one per port, labeled with port name.
- **Edges** are smooth bezier curves. Edges with a `when:` clause are visually marked (dashed or with a small condition icon at the midpoint). Hovering an edge with a condition shows a tooltip with the human-readable condition (e.g. `iter < 5 AND verdict ≠ PASS`).
- **Halt edges** (target = halt action) terminate at a small "🛑" or octagon icon, not a node.
- Panning, zooming, mini-map (bottom-right), fit-view button.

**Run metadata overlay** (floating card, top-right of the canvas, ~280px wide, semi-translucent or solid card with shadow):
- Run id (with copy button).
- Pipeline name + version.
- Start time, elapsed duration (live-updating if running).
- Variables in effect (collapsible accordion if many).
- Global status with the colored icon.
- Action buttons: "Open Manager", "Cancel run" (only if running), "Cleanup" (only if terminal).

The currently selected node in the canvas should be visually distinct (thicker border, slight glow). Selection drives the right panel content. Multiple nodes can be highlighted as "running" simultaneously (parallel fan-out is normal).

### 5.3 Right Panel — Selected Node detail (~360px wide, resizable)

Header:
- Node name + type badge.
- Status badge with current state.
- Iteration counter if relevant (`iter 2`).

Body, vertical scroll, sections:

**Terminal preview** (top, ~40% of panel height):
- Renders the latest content of the node's tmux session as ANSI-colored read-only text in a monospace block.
- Auto-refreshes every ~1-2 s.
- "Open terminal" button (top-right of preview): opens a native OS terminal window attached to this tmux session for real interaction.
- For interactive nodes that are awaiting user, show a prominent yellow banner above the preview: "This node is waiting for you. Click Open terminal to interact, then click Mark complete here when done." A "Mark complete" button is right under the banner.

**Inputs** (collapsible section):
- For each input port: port name, status (resolved / pending / accumulating), and the path of the file(s) it reads.
- "Open" link next to each path → renders the markdown content in an inline preview (modal or sheet).

**Outputs** (collapsible section):
- For each output port: port name + the path the agent must write.
- If the file exists: show a small green checkmark, file size, and "Open" link to preview.
- If missing: gray placeholder.
- For ports with declared frontmatter schema: show the schema, and if the file exists, show parsed frontmatter values inline (e.g. `verdict: PASS`).

**Initial prompt** (collapsible section):
- Shows what the agent received as system context: deterministic preamble (input paths, output paths, CLI commands available, iter, variable values) followed by the user-defined role prompt.
- Read-only. Useful for debugging "what did the agent see?".
- Monospace, syntax-highlighted markdown.

---

## 6. Run Mode — Pipeline Manager Access

### 6.1 Pipeline Manager

Each Run has a dedicated "manager agent" — a Claude Code session running in tmux, pre-loaded with the run's context, that can answer questions about the run and execute commands (`extend_cycle`, `resume_run`, `kill_node`, `restart_node`, `mark_node_done`, `cleanup_run`, etc.).

The "Open Manager" button (in the run metadata overlay or right-click menu of a run) opens a **native OS terminal window** attached to the manager's tmux session. The user converses with the manager in that window. There is **no embedded chat in the UI** for v1.

The UI should make the distinction between two terminal-attach actions visually obvious:
- **Attach to NodeRun** (the right-panel "Open terminal" button) — talk to one specific agent.
- **Open Manager** (the metadata overlay button) — talk to the supervisor of the whole run.

Use distinct icons / labels / hover tooltips to keep these clearly separated in the user's mental model.

---

## 7. Run Mode — Modals

### 7.1 "+ New Run" modal (launched from left panel)

A centered modal dialog (shadcn `Dialog`):

- Title: "Launch new run".
- **Pipeline selector** (required): a `Select` dropdown listing all available pipelines, separated into two groups: "Repo pipelines" (top) and "User pipelines" (bottom), each with a small badge.
- **Input** (required): a large textarea (≥6 rows). Placeholder text: "Free-text prompt, an issue link (e.g. `https://github.com/owner/repo/issues/42`), or a mix."
- **Variable overrides** (optional): an `Accordion` collapsed by default, labeled "Variable overrides (using defaults)". When expanded, lists the pipeline's declared variables with their default values pre-filled in editable inputs. The user can override any. When collapsed, a small label shows how many overrides are active (e.g. "(2 overridden)").
- Footer buttons: "Cancel" (secondary), "Launch" (primary). On Launch, the modal closes, the new run appears at the top of the left panel with `running` status, and the user is auto-navigated to it.

---

## 8. Edit Mode — 3-Panel Layout

Same shell, different content.

### 8.1 Left Panel — Pipelines list

Header:
- Title "Pipelines".
- "+ New Pipeline" button.
- Tabs or filter for "Repo" / "User" / "All".

Each item:
- Pipeline name + small badge `repo` or `user`.
- Number of nodes (e.g. "5 nodes").
- Last modified relative time.
- Click → opens this pipeline in the center canvas (replacing or in a new tab — see §8.4).
- Right-click: "Duplicate", "Rename", "Delete".

### 8.2 Center Panel — DAG editor canvas

xyflow canvas in editable mode:

- Drag a node from a small floating palette ("+ Add node") on the canvas to create a new empty node.
- Right-click on a node: "Duplicate" / "Delete".
- Drag from one node's output port handle to another node's input port handle to create an edge.
- Click on an edge to select it; right-click for "Delete edge".
- Empty canvas state: a dashed-border centered card "Drag a node here, or click + Add node".

**Tabs** at the top of the canvas if multiple pipelines are open simultaneously. Each tab = one pipeline file. Close button per tab. Tab title shows the pipeline name with a small dot if there are unsaved changes (rare since auto-save, but external edits could show as dirty briefly).

**No blocking validation** — even invalid pipelines (orphan ports, unreachable nodes, missing variable references) are allowed. Optional: subtle visual hints (a small warning icon on a node with unwired input ports), but never blocking.

### 8.3 Right Panel — Inspector (context-sensitive)

The right panel adapts to the selection:

**If a Node is selected:**
- Section "Identity": id (auto-generated, editable), Name (display name).
- Section "Type": radio between `code-mutating` and `doc-only`. Tooltip explains: "code-mutating gets its own git worktree and can commit; doc-only reads the pipeline branch and only writes Blackboard artifacts."
- Section "Behavior": toggle `interactive` (with help text: "When true, this node pauses after spawning and waits for the user to interact via terminal and click 'Mark complete' in run mode.").
- Section "Prompt": a large textarea (≥10 rows, monospace, markdown) bound to the node's external prompt file (e.g. `prompts/<node-id>.md`). Auto-saved.
- Section "Inputs": list of input ports. Each row: port name (editable), `repeated` toggle (with tooltip: "When true, this port reads all artifacts matching `iter-*/<port>.md` instead of just the latest."). "+ Add input port" button.
- Section "Outputs": list of output ports. Each row: port name (editable), expandable "frontmatter schema" subsection (key-value pairs of field name → enum/string/int with allowed values for enum). "+ Add output port" button.

**If an Edge is selected:**
- Section "Source": dropdown `node` + dropdown `port` (constrained to source node's outputs).
- Section "Target": radio between "Node" (then dropdown `node` + dropdown `port`) and "Halt" (then a textarea for the halt message, with mention of substitution `{iter}`, `{verdict}`, `{variable}`).
- Section "Condition (`when:`)": optional builder. List of clauses, each a row: dropdown of available fields (`iter`, `$<variable>`, frontmatter fields of source port) + dropdown predicate (`eq`/`neq`/`lt`/`lte`/`gt`/`gte`/`in`/`not_in`) + value input (literal or `$<variable>` reference). Plus a top-level switch between AND (default) and any-of OR.

**If nothing is selected (or the canvas background is clicked):**
- Section "Pipeline": Name (text input), Description (textarea), Version (number).
- Section "Variables": list of variables. Each: name + type (int / float / string / boolean / list) + default value. "+ Add variable".
- Section "Config": toggle `auto_merge_resolver` (default on).

### 8.4 Multi-pipeline tabs

In Edit mode the center can host multiple tabs of open pipelines. Drag-drop from one tab's canvas into another (for copying nodes) is a nice-to-have but not required for the first pass.

---

## 9. Status Color Palette (suggestion)

A consistent palette across run icons, node borders, and badges:

- `running`: `blue-500` with subtle pulse animation
- `awaiting_user`: `amber-500`
- `done`: `emerald-500`
- `blocked`: `orange-500`
- `failed`: `red-500`
- `archived`: `gray-400`
- `pending` (not yet started): `gray-300`

Dark mode uses lighter shades of the same hues.

---

## 10. Empty States & Edge Cases to Design

- **No runs yet** (Run mode left panel): centered illustration + text "No runs yet. Create one to get started." + prominent "+ New Run" button.
- **No pipelines yet** (Edit mode left panel): "No pipelines yet. Create one or import a YAML." + "+ New Pipeline" button.
- **Daemon disconnected**: a banner across the top (red) "PDO daemon not reachable. Retrying…" with manual "Retry" button.
- **Run blocked with halt**: in the run metadata overlay, the halt message is rendered in an orange callout. The manager attach button is highlighted as the next action.
- **Merge conflict during fan-in**: a special node-level visual cue ("conflict resolver running" → "auto-resolved" → continues) to make this visible during a run.
- **Terminal preview offline** (if `tmux capture-pane` fails for any reason): show a placeholder "Preview unavailable. Click Open terminal to attach directly."

---

## 11. Out of Scope

Do **not** design:

- A login screen, account management, billing, or anything multi-user.
- Mobile or tablet layouts. Desktop-only, min width ~1200 px.
- A dedicated "logs" tab — logs live in the terminal preview and the manager session.
- A node template library / palette of pre-built nodes. PDO v1 ships zero pre-built node templates by intent.
- Embedded chat with the manager. Manager is in a native OS terminal, period.
- Notifications, sound, badges on the OS dock.
- Any "help / tutorial / onboarding" overlay. Assume the user has read the docs.

---

## 12. Visual Tone

- **Calm, dense, precise.** This is a tool a senior engineer uses for hours. No glossy gradients, no decorative illustrations beyond empty-state placeholders, no marketing language.
- **Monospace where data is shown** (paths, ids, frontmatter values, terminal preview). Sans-serif everywhere else.
- **Subtle motion**: 150-200 ms ease for hovers and transitions. Pulsing only on the "running" status indicator. No bouncing, no spinning, no shimmering.
- **High information density** but with breathing room around panel headers and section dividers.
- **Dark mode is the default**. Light mode is supported but second-class polish in v1.

---

## 13. Deliverables Expected from the Design Tool

Generate a clickable mockup with at least these screens / states:

1. Run mode, default — a Run is selected, currently running with 2 active nodes (one `code-mutating` Implementer, one `doc-only` Reviewer).
2. Run mode — a Run is selected, blocked at a halt (max iterations reached). Manager attach button highlighted.
3. Run mode — a Run is selected, awaiting user input on an interactive node. Right panel shows the awaiting-user banner and the Mark complete button.
4. Edit mode — an existing pipeline open in the canvas, one node selected, right panel shows the node inspector.
5. Edit mode — an edge with a `when:` clause selected, right panel shows the condition builder.
6. Edit mode — nothing selected, right panel shows the pipeline-level inspector with variables list.
7. "+ New Run" modal open with the variable overrides accordion expanded.
8. Empty state — first launch with no runs and no pipelines.

Include both dark and light mode for at least screen 1.
