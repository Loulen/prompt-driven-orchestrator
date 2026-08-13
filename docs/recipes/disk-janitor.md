# Recipe — unattended disk janitor (#128 Track A, #480)

**Problem.** Every Run forks one or more git worktrees under `.pdo/runs/<run-id>/`.
A worktree of a JS repo carries a full `node_modules` (~1 GB); a code-mutating
node recompiles into its own `target/`. A machine that fires Triggers around the
clock accumulates terminal-Run residue and slows daemon startup (recursive inotify
watch setup over the accumulated checkouts).

**Why the runtime does not just delete them.** Deleting a `pdo/run-<id>` branch is
**irreversible** and destroys the run's only copy of its work — the same effect
class as merge/PR. **ADR-0012(a)** forbids the *runtime* from originating any
durable action: *« le runtime ne déclenche jamais d'action durable de lui-même »*.
So PDO does **not** auto-cleanup (`CONTEXT.md`: « Pas d'auto-cleanup, jamais »).
The fix keeps the **origin** of the deletion in a **pipeline** (versioned,
auditable), exactly where ADR-0012 says autonomy belongs — and fires it
unattended via a cron Trigger.

Two pieces, **both now shipped as tracked artifacts** (they used to be prose here;
#480 built them):

1. **`GET /runs/reapable`** — the runtime *surfaces* candidates (read-only, never
   deletes). Lists every **terminal, non-`archived`** Run whose worktree(s) are
   still on disk. (`list_reapable_runs`, `crates/pdo-daemon/src/lib.rs`.)
2. **The `disk-janitor` pipeline + a cron Trigger** — a one-node **`script`**
   pipeline (`.pdo/pipelines/disk-janitor.yaml` + its `.prompts/reap.md` body) runs
   `pdo reap`, which applies a graded-TTL policy and reclaims each match via the
   existing `cleanup_run` command. The Trigger fires it on a schedule so the
   residue is handled even when nobody is watching.

Why a `script` node + Rust and **not** the earlier `doc-only` + `python3` sketch:
the sandbox image ships **neither `jq` nor `python3`**, so a bash/JSON prompt is
dead on arrival there; and a `doc-only` node spends an LLM turn on a purely
mechanical task and is non-deterministic. A `script` node running deterministic
Rust (`pdo reap`) is testable end-to-end in CI with **zero stubbing** (ADR-0017),
and the policy itself is a pure, unit-tested function (`reap_policy`).

---

## 1. The surfacing endpoint

```bash
curl -s "$PDO_DAEMON_URL/runs/reapable"          # fast: no sizes
curl -s "$PDO_DAEMON_URL/runs/reapable?size=true" # walks each tree for bytes
```

Each entry:

| field | meaning |
|-------|---------|
| `run_id` | the Run |
| `pipeline_name` | which pipeline produced it |
| `status` | `completed` \| `failed` \| `halted` \| `skipped` (read-only surfacing does **not** pre-filter — the policy below decides) |
| `completed_at` | ISO timestamp of the terminal transition |
| `age_secs` | seconds since `completed_at`, computed server-side (the policy applies its TTL against this) |
| `worktree_present` | always `true` on a listed entry |
| `effective_repo` | repo the worktree lives under |
| `approx_disk_bytes` | only with `?size=true` (opt-in; walks the tree) |

Live runs (`running`/`awaiting_user`/`paused`) and already-`archived` runs
**never** appear.

---

## 2. `pdo reap` — the mechanical half

```bash
pdo reap --dry-run     # show the plan, delete nothing
pdo reap --count       # print just the count of policy matches (for a guard)
pdo reap               # reclaim: archive each match via cleanup_run
```

`pdo reap` lists `GET /runs/reapable?size=true`, runs the pure `reap_policy`
over it, and (unless `--count`/`--dry-run`) archives each selected Run with
`POST /runs/{id}/commands {"kind":"cleanup_run"}`. It talks to
`$PDO_DAEMON_URL` (injected into every node session; falls back to the built-in
default — set it explicitly for manual/prod use, e.g. `http://localhost:6160`).

**Graded TTL policy** (`reap_policy::ReapPolicy`, all overridable):

| category | default TTL | flag | rationale |
|----------|-------------|------|-----------|
| `completed` | 24 h | `--ttl-hours` | pure residue |
| `failed` / `halted` / `skipped` | 72 h | `--terminal-ttl-hours` | post-mortem evidence — held longer, but **bounded** (excluding it outright leaks those worktrees forever) |
| the janitor's own `disk-janitor` Runs | 1 h | *(built-in)* | an hourly cron leaves one completed janitor Run per fire; a short self-TTL stops them (and their `__manager__` sessions) piling toward the ~30-session tmux collapse |

Reclaims are ordered **biggest-first** and issued within a **wall-clock budget**
(`--budget-secs`, default 40 s). This matters: `cleanup_run` is synchronous and a
`script` node is killed at `SCRIPT_TIMEOUT_SECS` (60 s), so past the budget the
janitor stops starting new reclaims and **still exits 0**, leaving the rest for the
next fire — because a *failed* janitor Run leaves its own worktree behind, which a
`completed`-only policy would never reclaim (monotone residue). Only a total
inability to *list* is fatal (a silent no-op forever is worse than a visible
failure). A `200` is not taken as proof: `pdo reap` re-lists afterwards and reports
how many reclaims are **confirmed gone**.

Under disk pressure, tighten from the Trigger without touching code:
`pdo reap --ttl-hours 1`.

---

## 3. The cron Trigger (unattended) — an ops step

The pipeline reaches prod only through the tracked repo, and a live daemon must be
told to run it. Both are **out-of-Run, human** steps (a node cannot `make update`
the prod daemon nor create a Trigger against it safely):

```bash
# 1. Deploy: pull the tracked pipeline + prompt into the prod checkout, restart.
#    (make update = git pull --ff-only in ~/.pdo/app; do it yourself, not from a Run.)
make update

# 2. Create the Trigger. A guard keeps a quiet machine from spawning empty Runs:
#    it fires (exit 0) only when at least one Run currently matches the policy.
curl -s -X POST "$PDO_DAEMON_URL/triggers" \
  -H 'content-type: application/json' \
  -d '{
    "name": "disk-janitor (hourly)",
    "pipeline_id": "disk-janitor",
    "target_repo": "<abs path to the target repo>",
    "cron": "0 * * * *",
    "overlap_policy": "skip",
    "guard_command": "[ \"$(pdo reap --count)\" -gt 0 ]"
  }'
```

- `cron: "0 * * * *"` — hourly (the scheduler minimum is effectively hourly).
- `overlap_policy: "skip"` — never start a second janitor Run while one is live.
- `target_repo` — **required** since 1.3.0 (a `POST /triggers` without it is a 400).
- `guard_command` — `pdo reap --count` prints the number of policy matches; the
  guard fires only when it is `> 0`, so a quiet machine spawns no empty Runs.

---

## 4. Doctrine & safety notes

- **The runtime never deletes.** `GET /runs/reapable` is read-only; the deletion's
  *origin* is the pipeline/CLI calling `cleanup_run` — the ADR-0012-blessed shape.
  Pinned by the `reaper_never_deletes_worktree` test.
- **The janitor cannot delete itself.** Its own in-flight Run is `running`, so it
  never appears on `/runs/reapable`. The self-TTL only ever touches its *past*,
  terminal Runs.
- **Evidence is held, not hoarded.** `failed`/`halted`/`skipped` worktrees are
  post-mortem evidence, kept on the longer `--terminal-ttl-hours`; `cleanup_run`
  removes the run dir (pane snapshots) but preserves the event log (the Run flips
  to `archived` and stays queryable via `GET /runs/<id>`).
- **`git worktree remove --force`** will yank a worktree a shell is `cwd`-inside.
  A days-scale TTL makes this a non-issue in practice.
- **Manual run, any time:** `pdo reap` (or `pdo reap --dry-run`) works by hand
  without a Trigger.

---

## 5. Out of scope here (tracked separately)

The disk-fill *incidents* (`.pdo/runs` spiking to tens of GB) are dominated by
`target/` directories inside **live** Runs — one per code-mutating sub-worktree, at
the same commit — which are **never** reapable by construction (`is_terminal()`).
The janitor closes the *terminal-Run residue* leak and removes the human from that
loop; it does **not** shrink a running build's footprint. Sharing a cargo build
cache across sub-worktrees (a `CARGO_TARGET_DIR`) is the lever there — tracked as
its own issue (#518), not addressed by this recipe.

Further responsiveness refinements considered and deferred (the graded TTL +
`--ttl-hours` override is sufficient for the residue this addresses):
a `df`-based pressure gauge that shortens the TTL as the disk fills, and firing on
the Run-completion event rather than a cron so a large completed Run is reclaimed
minutes (not up to an hour) after it finishes.
