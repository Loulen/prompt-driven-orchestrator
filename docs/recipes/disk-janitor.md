# Recipe — unattended disk janitor (#128, Track A)

**Problem.** Every Run forks one or more git worktrees under `.pdo/runs/<run-id>/`.
A worktree of a JS repo carries a full `node_modules` (~1 GB), so a machine that
fires Triggers around the clock fills its disk and slows daemon startup (recursive
inotify watch setup over the accumulated checkouts).

**Why the runtime does not just delete them.** Deleting a `pdo/run-<id>` branch is
**irreversible** and destroys the run's only copy of its work — the same effect
class as merge/PR. **ADR-0012(a)** forbids the *runtime* from originating any durable
action: *« le runtime ne déclenche jamais d'action durable de lui-même »*. So PDO
does **not** auto-cleanup (`CONTEXT.md`: « Pas d'auto-cleanup, jamais »). The fix
keeps the **origin** of the deletion in a **pipeline** (yours, versioned, auditable),
exactly where ADR-0012 says autonomy belongs — and fires it unattended via a cron
Trigger.

Two pieces:

1. **`GET /runs/reapable`** — the runtime *surfaces* candidates (read-only, never
   deletes). It lists every **terminal, non-`archived`** run whose worktree(s) are
   still on disk.
2. **A janitor pipeline + cron Trigger** — a one-node pipeline reads that list,
   applies *your* policy (e.g. `completed` and older than N days), and reclaims each
   via the existing `cleanup_run` command. The Trigger fires it on a schedule so the
   disk-fill is handled even when nobody is watching.

---

## 1. The surfacing endpoint

```bash
curl -s "$PDO_DAEMON_URL/runs/reapable" | python3 -m json.tool
```

Each entry:

| field | meaning |
|-------|---------|
| `run_id` | the Run |
| `pipeline_name` | which pipeline produced it |
| `status` | `completed` \| `failed` \| `halted` \| `skipped` (read-only surfacing does **not** pre-filter failures — *you* decide) |
| `completed_at` | ISO timestamp of the terminal transition |
| `age_secs` | seconds since `completed_at` (apply your TTL against this) |
| `worktree_present` | always `true` on a listed entry |
| `effective_repo` | repo the worktree lives under |
| `approx_disk_bytes` | only when called with `?size=true` (walks the tree, so it is opt-in; omitted by default to keep the listing fast) |

Live runs (`running`/`awaiting_user`/`paused`) and already-`archived` runs **never**
appear. To prioritise the biggest reclaims first:

```bash
curl -s "$PDO_DAEMON_URL/runs/reapable?size=true" \
  | python3 -c 'import sys,json; rows=json.load(sys.stdin); rows.sort(key=lambda r: r.get("approx_disk_bytes",0), reverse=True); print(json.dumps(rows, indent=2))'
```

---

## 2. The janitor pipeline

A single agent node that queries the endpoint, filters by *your* policy, and calls
`cleanup_run` for each match. Save it to the library (or drop the YAML in
`.pdo/pipelines/`).

`disk-janitor.yaml`:

```yaml
name: disk-janitor
version: "1.0"
prompt_required: false
nodes:
  - id: start
    name: Start
    type: start
    outputs:
      - { name: go }
  - id: reclaim
    name: Reclaim disk
    type: doc-only          # it calls the daemon API; it does NOT mutate the target repo
    inputs:
      - { name: go }
    outputs:
      - { name: report }
    prompt_file: reclaim.md
  - id: end
    name: End
    type: end
    inputs:
      - { name: result }
edges:
  - source: { node: start, port: go }
    target: { node: reclaim, port: go }
  - source: { node: reclaim, port: report }
    target: { node: end, port: result }
```

`disk-janitor.prompts/reclaim.md` (the agent's instructions — the daemon exports
`PDO_DAEMON_URL` into every node session):

````markdown
You are a disk janitor. Reclaim disk from old, **completed** Runs. Do NOT touch
failed/halted Runs (they are debugging evidence) and never delete anything the API
does not list as reapable.

Policy: reclaim only `status == "completed"` Runs older than **7 days**
(`age_secs > 604800`).

Run exactly this, then report what you reclaimed:

```bash
TTL=604800   # 7 days, in seconds — tune to taste
curl -s "$PDO_DAEMON_URL/runs/reapable" \
  | python3 -c '
import sys, json
for r in json.load(sys.stdin):
    if r["status"] == "completed" and (r.get("age_secs") or 0) > '"$TTL"':
        print(r["run_id"])
' \
  | while read -r RUN; do
      echo "reclaiming $RUN"
      curl -s -o /dev/null -w "  cleanup_run %{http_code}\n" \
        -X POST "$PDO_DAEMON_URL/runs/$RUN/commands" \
        -H 'content-type: application/json' \
        -d '{"kind":"cleanup_run"}'
    done
```

A `200` means reclaimed; a `409` means it was already archived (benign — treat as a
no-op). Write a one-line summary (how many reclaimed, total) to your `report` output,
then `pdo complete`.
````

> The janitor's *own* in-flight Run is `running`, so it can never appear on
> `/runs/reapable` — it cannot delete itself out from under its own feet.

---

## 3. The cron Trigger (unattended)

Fire the janitor on a schedule. A **guard** keeps a quiet machine from spawning empty
janitor Runs: the guard exits `0` (fire) only when at least one Run currently matches
your policy, and non-zero (skip) otherwise.

```bash
curl -s -X POST "$PDO_DAEMON_URL/triggers" \
  -H 'content-type: application/json' \
  -d '{
    "name": "disk-janitor (daily, 7d TTL)",
    "pipeline_id": "disk-janitor",
    "cron": "0 4 * * *",
    "overlap_policy": "skip",
    "guard_command": "test \"$(curl -s \"$PDO_DAEMON_URL/runs/reapable\" | python3 -c '"'"'import sys,json;print(sum(1 for r in json.load(sys.stdin) if r[\"status\"]==\"completed\" and (r.get(\"age_secs\") or 0)>604800))'"'"')\" != 0"
  }'
```

- `cron: "0 4 * * *"` — daily at 04:00. Five-field cron; the scheduler minimum is
  effectively hourly.
- `overlap_policy: "skip"` — never start a second janitor Run while one is live.
- `guard_command` — gates the fire on exit code (`0` = fire). Here it counts
  policy-matching reapable Runs and only fires when there is at least one. Without a
  guard the Trigger fires every period regardless; with one you trade a cheap guard
  subprocess per period for not spawning empty Runs. Keep the guard's TTL in sync with
  the prompt's `TTL`.

---

## 4. Doctrine & safety notes

- **The runtime never deletes.** `GET /runs/reapable` is read-only. The deletion's
  *origin* is your pipeline node calling `cleanup_run` — that is the ADR-0012-blessed
  shape (« l'autonomie est une propriété du pipeline »). The invariant is pinned by the
  `reaper_never_deletes_worktree` test.
- **`completed`-only by policy.** A `failed`/`halted` worktree is post-mortem evidence.
  The endpoint surfaces them (tagged by `status`) but the recipe's filter excludes them.
  Widen the filter only if you mean to.
- **`cleanup_run` is irreversible** for the worktree + branches, but **preserves the
  event log**: the Run flips to `archived` and stays queryable (`GET /runs/<id>`). It
  does remove the run dir, which holds the post-mortem **pane snapshots** — hence a
  days-scale TTL, not minutes.
- **`git worktree remove --force`** will yank a worktree a shell is `cwd`-inside. With a
  `completed`-only + days-TTL policy this is a non-issue in practice, but be aware.
- **Manual run, any time:** the same endpoint + `cleanup_run` work by hand without a
  Trigger — see the commands in §1–2.
