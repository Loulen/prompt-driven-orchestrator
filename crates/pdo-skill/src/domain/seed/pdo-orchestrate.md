---
name: pdo-orchestrate
description: Orchestrate child runs with PDO — create runs from this node session, wait for them with `pdo run wait`, and complete truthfully.
skill_version: 3
---

# Orchestrating with PDO

You are running inside a PDO **node session**. Your identity travels in the
environment: `PDO_RUN_ID` (your run), `PDO_NODE_ID` (your node),
`PDO_NODE_ITER` (your iteration) and `PDO_DAEMON_URL` (the daemon to talk to).
Every run you create from this session is **mechanically linked** to your run
and node — the daemon sets the parent itself; you never declare it.

## Creating a child run

Prefer the CLI (same session, one short command):

```bash
pdo run create <pipeline-name> --input "<what the child must do>"
```

The pipeline is **positional**. The child inherits your run's project by
default; steer it with `--target-repo <path>`, `--variables '{"key":"value"}'`
(one JSON object), `--name "<title>"`, `--skills a,b`. Out of a node session the
same command works, without a parent.

### Passing files and images to the child

Your inputs are files on disk — `PDO_INPUT_<PORT>` names each input port's
path (a file, or a directory for an image list). Hand them to the child with:

- `--input-file <path>`: the child's prompt read from a file (instead of
  `--input`) — e.g. the spec an upstream node wrote.
- `--image <path>` (repeatable): sent as `images`; the child's entry node sees
  them under `## Input Images`.
- `--file <path>` (repeatable): any other file, sent as `files`; listed under
  `## Input Files` and exposed on the entry node's input port.

```bash
pdo run create <pipeline-name> \
  --input-file "$PDO_INPUT_SPEC" \
  --image "$PDO_INPUT_PROTO"/*.png \
  --file "$PDO_INPUT_FIXTURES" \
  --name "Implement the prototype"
```

All attachments land in the child's `.pdo/artifacts/_input/`. One size budget
per run (default 50 MB, `max_attachments_mb` in Settings); a duplicate
filename or an over-budget upload is refused with a named error — rename or
attach less, then retry.

The HTTP surface is equivalent — the CLI is a thin client:

```bash
curl -X POST "$PDO_DAEMON_URL/runs" \
  -H "Content-Type: application/json" \
  -d '{"pipeline": "<pipeline-name>", "input": "<what the child must do>"}'
```

The response carries `{"run_id": "..."}`. A child is an **ordinary run**: its
own graph, its own worktree, its own harness. Recursive orchestration works —
a child may orchestrate in turn.

## Waiting for your children

Never poll on a timer. Block on the daemon instead:

```bash
pdo run wait --timeout 600
```

`pdo run wait` returns as soon as **one** child of this node becomes terminal
(`--all`: once every child is terminal). Its exit code tells you what happened:

- **0** — something settled: one JSON line per child on stdout —
  `{"run_id","name","status","reason","children_active"}`. Read the child's
  outputs (its artifacts, its `reason` when it failed), then decide: create a
  corrected child, move on, or `pdo fail` if the plan is dead.
- **2** — `--timeout` elapsed, nothing settled, nothing on stdout. **Not a
  failure**: call `pdo run wait` again (loop `while ! pdo run wait --timeout 600;
  do :; done` when your harness caps the length of one tool call).
- **0 with `{"noop":true,…}`** — no child of this node is active: you have
  nothing to wait for.
- **1** — the daemon is unreachable, or you are not in a node session.

A child that is itself `awaiting_user` (its agent asked its user a question)
does **not** wake you: PDO shows that wait on your node and your run — « child
run <name> is awaiting you » — and lifts it when the user answers. Keep waiting.

If you need the full picture, `curl -s "$PDO_DAEMON_URL/runs/$PDO_RUN_ID/children"`
lists every child with status, duration and cost — a read, not a wait.

## Completing truthfully

The linkage is strong (ADR-0064): this node completes only once **every** child
run is terminal.

- `pdo complete` is **refused** (`children_pending`, exit 3) while any child is
  still running: wait for them with `pdo run wait`, then complete again.
- When every child is terminal and none failed, the node **completes by itself**
  the moment you give your turn back — you may simply call `pdo complete`.
- A **failed** child parks this node `awaiting_user`: the decision is the
  user's — retry the child, or force the node through (« Mark complete »).
  Say what failed and stop; do not decide for them.

Never declare completion while work you spawned is still in flight; the daemon
enforces it, and the honest move is to wait, follow up, or fail loudly.

## Good practice

- Give each child a precise, self-sufficient input: it cannot see your context.
- Orchestrate wide, not deep, unless the work truly nests.
- Your children outlive your own gestures (stop, archive) — do not use that to
  abandon them; finish what you spawned or leave it in a decided state.
