# Scenario — `repo-explorer-pick`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent drives a **real browser**
> (Chrome DevTools MCP preferred; Playwright MCP fallback) plus **Bash** (to seed a
> deterministic directory tree on the real filesystem) and emits the verdict format
> below. Asserts #131: a loupe icon on the repo combobox opens a **filesystem
> explorer** that lists directories one level at a time (dirs only, dotfiles hidden,
> git repos flagged, symlinks flagged, alpha-sorted), navigates up/down, **picks a
> folder through the existing validation path** (green border + branch loading for a
> git repo; red border for a non-repo), **degrades gracefully** on an unreadable
> directory (inline error, breadcrumb kept, never a blank pane or crash), and that
> the **nested modal** behaves (backdrop-click and Escape close only the explorer,
> never the parent New Run modal).
>
> jsdom can't hit-test real click-bubbling or exercise the real filesystem, so this
> Layer-5 + the Playwright spec `frontend/e2e/repo-explorer-pick.spec.ts` are the only
> layers that exercise the *real* browse path end to end. Treat this scenario as the
> human-equivalent acceptance gate.

## Setup

- A PDO daemon running on the user's repo. Discover the port — commonly
  `http://127.0.0.1:5172` (debug dev), `6160` (installed prod), or `6172`; find it
  with `ss -ltnp | grep -i pdo` and use that base URL for both the browser and any
  API calls.
- Frontend reachable in a browser at that base URL (the daemon serves the embedded
  `frontend/dist`). **The build under test must already include the #131 changes** —
  if validating a local branch, rebuild the frontend and re-embed before driving the
  UI (the daemon serves the *embedded* bundle, not the vite dev server, unless you
  point the browser at a vite dev port).
- The agent runs as a **non-root** user (so `chmod 000` actually denies the daemon;
  if running as root, the EACCES step is inconclusive — note it as an anomaly, do not
  fail the run on it).

### Seed a deterministic directory tree (Bash)

The explorer browses the *real* filesystem, so seed a known tree the assertions can
bind to. Use a fixed path and recreate it idempotently:

```bash
ROOT=/tmp/pdo-l5-repo-explorer
rm -rf "$ROOT"; mkdir -p "$ROOT"
# a real git repo  → must show a git dot AND validate green
git init -q "$ROOT/alpha-project" && git -C "$ROOT/alpha-project" config user.email t@t.co \
  && git -C "$ROOT/alpha-project" config user.name t \
  && (cd "$ROOT/alpha-project" && echo hi > README.md && git add . && git commit -qm init)
# a plain directory (no .git) → no dot; pickable but validates RED
mkdir -p "$ROOT/beta-plain"
# a dotfile directory → MUST be hidden in the listing
mkdir -p "$ROOT/.hidden-dir"
# a symlink to a directory → listed, flagged as a symlink, navigable
ln -s "$ROOT/alpha-project" "$ROOT/zeta-link"
# an unreadable directory → navigating IN must degrade gracefully (EACCES)
mkdir -p "$ROOT/noaccess"; chmod 000 "$ROOT/noaccess"
# a plain file → MUST NOT appear (dirs only)
echo "notes" > "$ROOT/notes.txt"
ls -la "$ROOT"
```

Expected listing of `$ROOT` (case-insensitive alpha, dirs only, dotfiles hidden):
**`alpha-project`, `beta-plain`, `noaccess`, `zeta-link`** — and **not** `.hidden-dir`
(dotfile) nor `notes.txt` (file). `alpha-project` carries a git dot; `zeta-link`
carries a symlink marker.

Restore permissions in cleanup (`chmod 755 "$ROOT/noaccess"`) so `rm -rf` works.

## Steps the agent executes — open + listing

1. Open the UI; confirm the **`Daemon: connected`** label is visible in the status
   bar.
2. Open the **New Run** modal (the "New Run" / "+" affordance). Confirm the modal
   panel is visible (`data-testid="target-repo-input"` present).
3. **Open-at logic (Option B):** clear the repo input and type the seed root
   `/tmp/pdo-l5-repo-explorer` into `[data-testid="target-repo-input"]`. Then click
   the loupe trigger `[data-testid="repo-browse-trigger"]`.
4. The explorer modal `[data-testid="repo-browser-modal"]` appears. Assert its
   breadcrumb `[data-testid="repo-browse-path"]` shows `/tmp/pdo-l5-repo-explorer`
   (Option B opened at the typed absolute path, not `$HOME`).
5. **Listing correctness.** Read the entry rows `[data-testid="repo-browse-entry"]`:
   - Exactly **4** rows, in order: `alpha-project`, `beta-plain`, `noaccess`,
     `zeta-link` (case-insensitive alpha).
   - `.hidden-dir` is **absent** (dotfile hidden).
   - `notes.txt` is **absent** (files filtered; dirs only).
   - `alpha-project` shows a **git indicator** (the `is_git_repo` dot); `beta-plain`
     does **not**.
   - `zeta-link` shows a **symlink marker** (`is_symlink`).

## Steps — navigate + pick (the happy path through existing validation)

6. Click the `zeta-link` entry. Assert the explorer navigates **into** it (breadcrumb
   updates; because it points at the git repo, the listing is that repo's top level —
   e.g. empty of sub-dirs, which is fine). This proves a symlinked dir is navigable.
7. Click the **up** affordance `[data-testid="repo-browse-up"]` until the breadcrumb
   is back at `/tmp/pdo-l5-repo-explorer`. (Up is enabled because `parent != null`.)
8. **Pick a git repo:** click `alpha-project` to navigate into it, then click
   **Select this folder** `[data-testid="repo-browse-select"]`. (If the build also
   supports picking a git-dotted entry directly, that path is acceptable too — fork
   F2.) Assert:
   - The explorer closes; the **New Run modal stays open**.
   - The repo input value is now `/tmp/pdo-l5-repo-explorer/alpha-project`.
   - Within ~1s (existing 400ms debounce → `/repos/validate` → `/repos/branches`),
     the input border turns **green** and `[data-testid="repo-valid"]` ("Valid git
     repository") is visible.
   - The **Source branch** select `[data-testid="source-branch-select"]` appears and
     is populated (at least `main`). This proves the pick reused the existing
     validation/branch-loading flow with **no new logic**.
9. **Pick a non-git folder:** re-open the loupe (it opens at the current value's dir —
   `alpha-project`), click up to `/tmp/pdo-l5-repo-explorer`, navigate into
   `beta-plain`, **Select this folder**. Assert:
   - Input value is `/tmp/pdo-l5-repo-explorer/beta-plain`.
   - The border turns **red** and `[data-testid="repo-error"]` shows a
     "not a git repository" message (any folder is pickable — ADR-0001 — and the
     authoritative `git` validation gates it).
   - The Launch button `[data-testid="launch-button"]` is **disabled**.

## Steps — graceful degrade (unreadable directory)

10. Re-open the explorer and navigate to `/tmp/pdo-l5-repo-explorer`. Click the
    `noaccess` entry. Assert (non-root):
    - The explorer **does not crash or blank out**: an inline error
      `[data-testid="repo-browse-error"]` is visible (e.g. "permission denied").
    - The breadcrumb still shows the path (`.../noaccess` or stays at the parent —
      either is acceptable as long as the user is not stranded on a blank pane).
    - The New Run modal is **still open** and the explorer is **still usable** (click
      up returns to a readable listing). No uncaught exception reached the devtools
      console.
    - *(If running as root, EACCES won't trigger — record this step as
      `inconclusive (root)` in anomalies, do not FAIL.)*

## Steps — nested-modal interaction (must not close the parent)

11. With the explorer open, click its **backdrop** `[data-testid="repo-browse-backdrop"]`
    (empty space outside the panel). Assert: the **explorer closes** AND the **New Run
    modal remains open** (`[data-testid="target-repo-input"]` still visible). This is
    the click-bubble guard (`stopPropagation` on the `z-[60]` backdrop).
12. Re-open the explorer. Press **Escape**. Assert: the **explorer closes** AND the
    New Run modal **remains open**. Press Escape **again**: now the recents dropdown
    (if open) closes / nothing breaks — the parent modal must still be open (it has no
    Escape handler). This proves Escape is scoped to the top-most layer
    (`!explorerOpen` gate on the combobox listener).

## Cleanup

- `chmod 755 /tmp/pdo-l5-repo-explorer/noaccess && rm -rf /tmp/pdo-l5-repo-explorer`.
- Close the New Run modal (Cancel/X). No run was launched, so there is nothing to
  archive and no tmux session was spawned by this scenario.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "setup: seeded /tmp/pdo-l5-repo-explorer with alpha-project(git), beta-plain, .hidden-dir, zeta-link(symlink), noaccess(000), notes.txt",
    "step 1: status bar 'Daemon: connected'",
    "step 4: loupe opened explorer at typed path /tmp/pdo-l5-repo-explorer (Option B)",
    "step 5: 4 entries [alpha-project, beta-plain, noaccess, zeta-link]; .hidden-dir and notes.txt absent; alpha-project git-dotted; zeta-link symlink-marked",
    "step 6-7: zeta-link navigable (symlinked dir); up returns to root",
    "step 8: picked alpha-project -> input filled, green border + 'Valid git repository', source-branch populated (main) — reused existing validation",
    "step 9: picked beta-plain -> red border 'not a git repository', Launch disabled (any folder pickable, git gates)",
    "step 10: clicked noaccess -> inline 'permission denied', no blank pane, no console exception, modal still open",
    "step 11: explorer backdrop click closed explorer only; New Run modal stayed open",
    "step 12: Escape closed explorer only; parent modal stayed open"
  ],
  "anomalies": [
    "<optional — e.g. running as root so step 10 EACCES inconclusive; truncation banner if seed dir unexpectedly large; symlink marker styling nit>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass. The
non-negotiable assertions are: **step 5** (dirs-only + dotfiles-hidden + correct
flags — the listing contract), **step 8** (pick reuses the existing green-border /
branch-loading validation, no new logic), **step 10** (graceful degrade, never a
crash/blank — the EACCES case is the one the explorer hits constantly in real use),
and **step 11/12** (the nested modal never closes the parent). Any of those failing
is an automatic `FAIL` regardless of the happy path.
