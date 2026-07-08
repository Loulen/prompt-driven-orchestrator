# ADR-0023 — Detached run-advance after terminal node transitions

## Status

Accepted (ratified via Discord, 2026-07-03 — issue #304, Option A "DETACH").

## Context

A node signals its terminal transition through the in-session `pdo` CLI
(`complete` / `fail` / `skip`), a `reqwest::blocking` one-shot running **inside
the node's own tmux session**. The daemon's handler (`node_done`, `node_fail`,
`node_skip`) appends the terminal event, then calls `reap_node_session` — which
kills that very tmux session, i.e. the HTTP client of the in-flight request —
and only then runs the rest of the work (successor spawn via
`run_advance::complete_node`, `RunFailed`/`RunSkipped` append,
`retry_waiting_nodes`).

hyper 1.x (`half_close = false`) cancels the in-flight handler future at its
next `.await` once the client socket closes. A cancelled future executes no
`catch` and logs nothing: the successor spawn (or the end-port finalization /
`RunCompleted`) is silently dropped. Only the 148 s idle reconciler (#279)
notices — and the end-port variant escapes even that (run stuck `running` with
all nodes completed, observed on `20260704-100029-e3e5ac3`). Five production
recurrences were logged between 2026-07-02 and 2026-07-04; the bug even blocked
its own fix from landing through the autonomous pipeline.

A reorder-only fix (reap after advance) removes just the self-inflicted
disconnect, not the class: any client disconnect mid-advance (crash, network,
manual Ctrl-C on `pdo complete`) still cancels the advance, and no deterministic
regression test can go green under it.

## Decision

After the terminal event (`NodeCompleted` / `NodeFailed` / skip marker) is
durably appended, the remainder of the handler — session reap plus every
state-advancing step — runs on a **detached `tokio::spawn` task**, decoupled
from the HTTP request future. The handler returns its response immediately
after spawning the task. Applies to all three CLI-facing terminal handlers:

- `node_done`: reap + `run_advance::complete_node` (edge firing, successor
  spawn, end-port finalization, `retry_waiting_nodes`, completion gate).
- `node_fail`: reap + `RunFailed` append + `retry_waiting_nodes`.
- `node_skip`: reap + `RunSkipped` append + `retry_waiting_nodes`.

`handle_merge_resolver_done` and the `mark_node_done` command are **not**
detached: neither reaps its own caller, so they have no self-cancellation
window, and detaching them would only trade response-visible errors for
fire-and-forget ones.

The detached task is wrapped in panic isolation (`catch_unwind` over the tail):
a panic is logged **and** surfaced as a `RunFailed` event
(`{"reason": "run advance panicked after &lt;event&gt; for &lt;node&gt;: …"}`), because a
panic landing after the successor's `NodeStarted` or inside the completion gate
falls outside the #279 reconciler's coverage.

## Consequences

- **Contract change:** `pdo complete` (and `fail`/`skip`) receives its 2xx
  **before** the run has advanced. The 2xx means "your terminal event is
  durably recorded and the advance is scheduled", not "the run has advanced".
  Advance errors surface via `RunFailed` + daemon logs, never via the HTTP
  response. Validation errors (transition-guard reject/no-op, merge conflicts,
  output-validation failures, append failures) still return in-request.
- A client disconnect at any point can no longer cancel the advance — closing
  the whole silent-abort class upstream of #279's guard, including the
  end-port finalization drop.
- The detached task is untracked (fire-and-forget), consistent with the
  daemon's existing background fleet (reaper, stale detector, trigger
  scheduler — all unjoined `tokio::spawn`). If the daemon dies between the 2xx
  and the advance, boot recovery + the stall reconciler handle the wedged run
  (reconcile-to-Failed, not resume) — same exposure as today, minus the
  in-request window.
- Concurrency is safe by existing construction: no lock is held across the
  tail today (`merge_lock` is scoped to the merge block), and spawning is
  idempotent (transition guard #212 + `compute_ready_to_spawn`), so a detached
  advance racing `re_evaluate_after_command` cannot double-spawn.
- Testability: this is the only shape under which the deterministic regression
  test (client drops the TCP connection mid-window → successor still spawns /
  run still completes) goes green; it stays red under reorder-only.
