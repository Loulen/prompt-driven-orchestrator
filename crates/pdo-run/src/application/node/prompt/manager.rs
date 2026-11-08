//! The Pipeline Manager's prompt, and whether a new Run is auto-named.

/// Middle tier of `stored → env → default(true)`; resolved by
/// [`default_auto_name_with`].
pub const DEFAULT_AUTO_NAME_ENV: &str = "PDO_DEFAULT_AUTO_NAME";

/// Built-in default for Run auto-naming: **on** — a Run created with no name is
/// auto-named by the manager.
pub const DEFAULT_AUTO_NAME_DEFAULT: bool = true;

/// Reuses the shared boolean parser so a typo falls through to the next tier
/// rather than silently meaning `false`.
pub fn env_default_auto_name() -> Option<bool> {
    std::env::var(DEFAULT_AUTO_NAME_ENV)
        .ok()
        .as_deref()
        .and_then(crate::application::stale::detector::parse_bool_setting)
}

/// Resolve the instance default for auto-naming: `stored → env → default(true)`.
///
/// `stored` is the raw `instance_config.default_auto_name` column: `Some(0)` is a
/// stored **off** and wins over the env; only SQL `NULL` falls through.
///
/// Only ever the *default* — the create-run chokepoint consults it solely when
/// the request carries neither an explicit `auto_name` flag nor a `name`.
pub fn default_auto_name_with(stored: Option<i64>) -> bool {
    match stored {
        Some(v) => v != 0,
        None => env_default_auto_name().unwrap_or(DEFAULT_AUTO_NAME_DEFAULT),
    }
}

/// How the manager should treat run naming, decided by the daemon at spawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunNameHint {
    /// The user supplied a display name — do not rename.
    UserProvided,
    /// No name, but there is input to summarise — name it now from `_input`.
    DeriveFromInput,
    /// No name and no input; a deterministic placeholder was set. Rename best-effort,
    /// only once enough is known, never by polling.
    Placeholder,
}

pub fn build_manager_preamble(run_id: &str, daemon_url: &str, name_hint: RunNameHint) -> String {
    let auto_name_instruction = match name_hint {
        RunNameHint::UserProvided => String::new(),
        RunNameHint::DeriveFromInput =>
            "\n**No display name was provided for this run.** As your first action, read the user input from the `_input` artifact and issue a `rename_run` command with a short, descriptive name (2–5 words) that captures the intent of the run.\n".to_string(),
        RunNameHint::Placeholder =>
            "\n**This run has a placeholder display name** (`Untitled run …`) because it started without a prompt — its real purpose only becomes clear once its nodes do work. Do **not** rename it as your first action, and do **not** poll or block waiting for nodes. Instead, *when you have enough context to know what this run is actually doing* (typically once nodes have produced output, e.g. when the user later engages you), give it a short, descriptive name (2–5 words) via the `rename_run` command. This is best-effort: if you never gain enough context, leave the placeholder in place.\n".to_string(),
    };

    format!(
        r#"# Pipeline Manager Runtime Preamble

You manage **run `{run_id}`**.
{auto_name_instruction}
- Daemon base URL: `{daemon_url}`
- Run state: `curl {daemon_url}/runs/{run_id}`
- Event log: `curl {daemon_url}/runs/{run_id}/events`
- Node pane: `curl {daemon_url}/runs/{run_id}/nodes/<node-id>/pane?iter=<N>`
- Node IO: `curl {daemon_url}/runs/{run_id}/nodes/<node-id>/io?iter=<N>`
- Artifact: `curl '{daemon_url}/runs/{run_id}/artifact?path=<relative-path>'`

A node's IO also lists its **live artifacts** (`live`): files its agent publishes in the
`live/` folder of its iteration while it works, so the humans can follow along. They are not
deliverables: the completion never checks them, no edge carries them, a skip writes nothing
for them, and they are not ports to force with `overrides`. A declared one that is missing is
"not produced", never a fault.

## Why a run is not advancing — read the reason, never `journalctl`

The runtime never fails a run on its own initiative (ADR-0049): any non-advancement — an
infra incident (`Interrupted` node), a runtime give-up (stall, output-validation refusal,
merge conflict), or an `unrouted` region/convergence — parks the run `awaiting_user` and
records **why** in the run state itself. You never need the daemon's logs to know the cause.

On `curl {daemon_url}/runs/{run_id}` a parked run carries:

- **`awaiting_reason_code`** — a stable machine slug you branch on: `session_died`,
  `spawn_aborted`, `boot_recovery` (an infra incident on a node); `run_stalled` (nothing
  schedulable); `unrouted` / `region_exhausted` / `region_ended_unrouted` (routing left no
  live path — route it with `end_region`/`bump_region` or the exit edge);
  `region_budget_exhausted` (a loop reached its `max_cost_usd` cost cap at re-entry, its exit
  NOT followed — `awaiting_region_id` names it; raise the cap with `set_region_max_cost`, `force_route` or `end_region`);
  `merge_conflict`,
  `merge_resolution_failed`, `script_validation_failed`, `frontmatter_retry_exhausted`,
  `delivery_failed` (a completion give-up); `agent_fail_awaiting` (an agent `pdo fail`
  awaiting your confirmation).
- **`awaiting_reason`** — the same cause in prose, for the human.

`awaiting_reason_code` **absent** on an `awaiting_user` run means the wait is **declared**
(a node's agent asked its user a question with `pdo wait-user`, or waits for the completion
release; `awaiting_reason` then carries the question — or « child run <name> is awaiting
you » when the wait is a child run's), not an incident — leave it be. Per node, an
interrupted node carries the same cause in `nodes.<id>.failure_reason`. Recover with the
lever the code points to (`restart_node`, `bump_region`/`end_region`, a fix + reopen); the
targeted commands re-open the run themselves.

## Available commands

All commands are issued via `POST {daemon_url}/runs/{run_id}/commands` with a JSON body.

### 1. bump_region

Grant a bounded loop region N more iterations and re-evaluate. This is THE lever for any node that belongs to a loop region — never `extend_cycle`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"bump_region","region_id":"<region-id>","additional_iter":<N>}}'
```

**Finding the `region_id`:** it is a key of `loop_states` in `curl {daemon_url}/runs/{run_id}` (the `loop_node_id` of `loop_iter_started` events). A bounded region has a `loop_states` entry **from lap 1** (#601), so "no entry" means "no such loop", not "first lap" — an absent key is a genuine miss to read the pipeline definition's `loops:` block for, not a first-lap blind spot. An unknown `region_id` is rejected with 400 before anything is recorded.

The response tells you what actually happened: `{{"ok":true,"spawned":[…]}}` when nodes were re-launched, or `{{"ok":true,"noop":true,"reason":"…"}}` when nothing was eligible yet (e.g. the region's current iteration is still running — the extra laps then apply when it finishes).

### 2. end_region

Fire a bounded loop region's completion now (route its exit) instead of running more laps. Same `region_id` discovery and same truthful response body as `bump_region`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"end_region","region_id":"<region-id>"}}'
```

### 3. set_region_max_iter / set_region_max_cost

Edit a bounded loop region's caps **in flight**, without editing the pipeline or restarting. Both are **absolute** (the new total, not a delta), last write wins, and the override is kept in the event log (it survives a reopen) and beats the declared value, a literal or a `$var` alike.

- `set_region_max_iter`: the region's iteration cap (`max_iter`, a positive integer).
- `set_region_max_cost`: the region's **cost cap** in dollars (`max_cost_usd`): a positive amount, or `null` to **remove** the cap. **No floor**: a cap below what the loop already spent is accepted; the lap in progress always finishes, and the loop stops (« budget reached ») at its next re-entry. Refused (400, nothing recorded) on a `collection` region, for a value ≤ 0 and for an unknown region.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"set_region_max_cost","region_id":"<region-id>","max_cost_usd":<USD>|null}}'
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"set_region_max_iter","region_id":"<region-id>","max_iter":<N>}}'
```

On a run parked `region_budget_exhausted` (or `region_exhausted`), the command lifts the park itself and re-checks the re-entry: raising the cap starts the next lap at once, a cap still under the spend parks it again. The loop's spend and its effective cap are in `loop_costs.<region-id>` (`usd`, `cap_usd`) of `curl {daemon_url}/runs/{run_id}`, the live overrides in `region_max_cost_overrides`. Same `region_id` discovery and same truthful response body as `bump_region`. From a shell: `pdo run region <run> <region> --max-cost <usd|none>` (`--max <n>` for the lap cap).

### 4. resume_run

Re-run the scheduler from the current state. Use after a manual conflict resolution or after extending a cycle on a halted run.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"resume_run"}}'
```

### 5. kill_node

Kill a running NodeRun's tmux session and emit `node_failed`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"kill_node","node_id":"<node-id>","iter":<N>}}'
```

### 6. restart_node

Kill a NodeRun and re-spawn it on the **same iter** with a new session. On an isolated node the sub-worktree is **reused in place**, so the dead session's uncommitted work is still there.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"restart_node","node_id":"<node-id>","iter":<N>}}'
```

The response tells you what actually happened; it never blanket-claims success (#489, ADR-0037). `200 {{"ok":true,"spawned":[{{"node_id":"…","iter":N}}],"reused_sub_worktree":<bool>,"base_sha":"<sha>"|null,"interrupted_git_ops":["index.lock",…]|[]}}` when the node was re-spawned. The re-spawned agent is told **directly in its own preamble** what to do about a reused worktree and any interrupted git operation left in it, so you do not need to relay instructions — `reused_sub_worktree` and `interrupted_git_ops` are there for your situational awareness (`interrupted_git_ops` lists every marker found, in order — `index.lock`, `MERGE_HEAD`, `rebase-*` — and is `[]` when the reused worktree's git state was clean). `200 {{"ok":true,"waiting":true,"reason":"…"}}` when the session cap queued it: a `NodeWaiting` **was** recorded and the admission sweep owns it — it will spawn, do not re-issue. Otherwise: `409 {{"error":"<slug>","recoverable":<bool>, …}}` with `restart_refused`, `sandbox_prep_not_ready` or `sub_worktree_occupied`; `400 {{"error":"node_not_found"}}`; `500 {{"error":"spawn_failed","run_failed":<bool>}}`. Discriminate on `error`, never on the status.

Every knowable refusal is raised **before** the session is killed: an error body with `session_killed:false` means nothing was touched, so fix the cause and re-issue. `session_killed:true` means the session is gone and nothing replaced it — that node needs a different lever, not a retry of this one.

### 7. mark_node_done

Force-complete a NodeRun (typically an interactive node the user has finished with).

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"mark_node_done","node_id":"<node-id>","iter":<N>}}'
```

This goes through the same shared completion body as `pdo complete`, so it answers truthfully (#490): `200 {{"ok":true}}` when the node completed, `200 {{"ok":true,"noop":true,"reason":"…"}}` on a legal duplicate, and **`409 {{"error":"<slug>","recoverable":<bool>, …}}`** when the completion is refused — `missing_outputs`, `frontmatter_retry_pending`, `frontmatter_retry_exhausted`, `script_validation_failed`, `completion_rejected`, … Discriminate on `error`, never on the status. `recoverable:false` means the runtime already recorded the terminal event: do not try to record it again.

### 8. release_node_completion

Release the daemon completion guard for one live interactive NodeRun. The agent may then call `pdo complete` when ready. This is idempotent and does not inject text into the node session.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"release_node_completion","node_id":"<node-id>","iter":<N>}}'
```

Returns `200 {{"ok":true,"released":true}}`, with `"noop":true` when already released. Named `409` refusals include `node_not_interactive` and `node_session_not_live`; discriminate on `error`, never on the status.

### 9. inject_artifact

Write an artifact directly into the Blackboard.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"inject_artifact","path":"<node-id>/iter-<N>/<port>/output.md","content":"<markdown content>"}}'
```

### 10. cleanup_run

Archive the run: remove worktrees, branches, and artifacts from disk. Events are preserved.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"cleanup_run"}}'
```

**Never call `cleanup_run` on your own initiative.** It is destructive and irreversible: it kills every active node session and removes the run's worktrees, branches, and artifacts from disk. Always check with the user first and wait for explicit confirmation before issuing it — even if you believe the run is stuck or finished.

### 11. rename_run

Set or update the display name of this run.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"rename_run","name":"<display name>"}}'
```

### 12. start_node

Force-spawn a node now, without waiting for its upstream producers to complete. Use when you deliberately want to start a node ahead of its dependencies. Inputs resolve best-effort: any not-yet-produced upstream artifact resolves to the path where it *will* appear, so the node may run against missing or stale inputs. This is reversible — `restart_node` or `kill_node` it if it ran too early.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"start_node","node_id":"<node-id>"}}'
```

### 13. extend_cycle (legacy)

Increment the iteration ceiling of a *legacy drawn cycle* (a pipeline without a `loops:` block) and re-evaluate. `node_id` is the node whose **outgoing exit condition** references the `$max_iter`-style variable to bump — never the cycle's head/entry node. For any node that belongs to a bounded loop region this command is rejected with 409 — use `bump_region` instead. An unknown `node_id` is rejected with 400. Same truthful response body as `bump_region`.

```bash
curl -X POST {daemon_url}/runs/{run_id}/commands \
  -H 'Content-Type: application/json' \
  -d '{{"kind":"extend_cycle","node_id":"<node-id>","additional_iter":<N>}}'
```

---

"#
    )
}

pub fn build_manager_prompt(
    run_id: &str,
    daemon_url: &str,
    role_prompt: &str,
    name_hint: RunNameHint,
) -> String {
    let preamble = build_manager_preamble(run_id, daemon_url, name_hint);
    format!("{preamble}{role_prompt}")
}
