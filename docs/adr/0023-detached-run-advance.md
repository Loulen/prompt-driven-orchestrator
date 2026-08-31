# ADR-0023 — Detached run-advance after terminal node transitions

## Status

Accepted (ratified via Discord, 2026-07-03 — issue #304, Option A "DETACH").

## Context

Without this ADR, the natural shape is to finish the terminal-transition handler inline — and the
handler kills the very tmux session hosting its own HTTP client (`pdo complete` runs *inside* the
node's session). hyper 1.x (`half_close = false`) then cancels the in-flight handler future at its
next `.await`. A cancelled future runs no `catch` and logs nothing: the successor spawn (or the
end-port finalization / `RunCompleted`) is silently dropped. Only the 148 s idle reconciler (#279)
notices — and the end-port variant escapes even that. Five production recurrences between
2026-07-02 and 2026-07-04; the bug blocked its own fix from landing.

A reorder-only fix (reap after advance) removes just the self-inflicted disconnect, not the class:
any client disconnect mid-advance (crash, network, Ctrl-C) still cancels the advance, and no
deterministic regression test can go green under it.

## Decision

After the terminal event is durably appended, the remainder of the handler — session reap plus
every state-advancing step — runs on a **detached `tokio::spawn` task**, decoupled from the HTTP
request future. The handler returns its response immediately. Applies to the three CLI-facing
terminal handlers (`node_done`, `node_fail`, `node_skip`).

`handle_merge_resolver_done` and the `mark_node_done` command are **not** detached: neither reaps
its own caller, so they have no self-cancellation window, and detaching them would only trade
response-visible errors for fire-and-forget ones.

The detached task is wrapped in panic isolation: a panic is logged **and** surfaced as a `RunFailed`
event, because a panic landing after the successor's `NodeStarted` or inside the completion gate
falls outside the #279 reconciler's coverage.

## Consequences

- **Contract change:** `pdo complete` (and `fail`/`skip`) receives its 2xx **before** the run has
  advanced. The 2xx means "your terminal event is durably recorded and the advance is scheduled",
  not "the run has advanced". Advance errors surface via `RunFailed` + daemon logs, never via the
  HTTP response. Validation errors (transition-guard reject, merge conflicts, output-validation,
  append failures) still return in-request — and since ADR-0035 (#490) they return **non-2xx**. The
  one exception is the legal-duplicate no-op, which grants nothing and stays a `200` on purpose.
- A client disconnect can no longer cancel the advance — closing the silent-abort class upstream of
  #279's guard, including the end-port finalization drop.
- The detached task is untracked, consistent with the daemon's existing background fleet. If the
  daemon dies between the 2xx and the advance, boot recovery + the stall reconciler handle the
  wedged run (reconcile-to-Failed, not resume) — same exposure as today, minus the in-request window.
- Concurrency is safe by existing construction: no lock is held across the tail, and spawning is
  idempotent (transition guard #212), so a detached advance racing `re_evaluate_after_command`
  cannot double-spawn.
- This is the only shape under which the deterministic regression test (client drops the TCP
  connection mid-window → successor still spawns) goes green; it stays red under reorder-only.
