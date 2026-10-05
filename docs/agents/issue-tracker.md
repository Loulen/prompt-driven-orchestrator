# Issue tracker: PDO's Project extension (`pm`)

Tickets and specs live in **`pm`**, the Project extension of the `pdo-dist` instance, in the
Backlog **`CORE`**, the same Backlog as the business stories (`docs/agents/business-backlog.md`),
with `type=tech`. Use the `pm` skill (`pdo ext pm …`) for all operations, never `gh issue`. If
`pdo ext` is unknown, the `pdo` on the PATH is not the Distribution: call
`$(brew --prefix pdo-dist)/bin/pdo`.

## Conventions

- **Create a ticket**: `pdo ext pm tache create "<titre>" --backlog CORE --set "type=tech" --status "To do" --body-file ticket.md`
  (keep `--json | jq -r .reference` to chain). Every technical ticket carries `type=tech`.
- **Read a ticket**: `pdo ext pm tache show CORE-12` then `pdo ext pm commentaire list CORE-12`.
- **List tickets**: `pdo ext pm tache list --backlog CORE --status "To do"` (or `--vue <Vue>`).
- **Comment**: `pdo ext pm tache comment CORE-12 -` and a heredoc.
- **Blocking edges**: `pdo ext pm tache block CORE-13 --by CORE-12` (the native Relation « Blocked by »).
- **Change stage**: `pdo ext pm tache update CORE-12 --status "In progress" | "In review" | Done`.
- **Wontfix**: comment why, then `pdo ext pm tache delete CORE-12` (to the Corbeille).

## Spec and parent story

- `/to-spec` writes the spec as a **sub-page of the story**:
  `pdo ext pm page create "Spec : <titre>" --parent CORE-8 --body-file spec.md`.
- `/to-tickets` creates the tickets, then adds each one to the story's Relation `tasks`
  (`pdo ext pm tache update CORE-8 --set "tasks=CORE-9,CORE-10,…"`) and comments the list on the story.
- In a ticket body, cite the story and the spec by their ID / Lien de page (`[[CORE-8]]`); outside
  `pm` (PR, artefact), write the ID **and** its deep link (`lien` of `--json`).

## Closing: no `Closes #N`

A merge closes nothing by itself. The node that validates a ticket sets it to **Done** once its
merge is **on the remote** (`main` pushed, or the integration branch). The story is set to **Done**
when the integration branch is merged into `main`.

## Legacy GitHub Issues

The core's issues still open on `Loulen/PDO-v2` stay on GitHub until they are closed (read one with
`gh issue view <number> --comments` when a prompt names it). New core work goes to `CORE`.

## Extensions live elsewhere

A ticket that only concerns an extension or `pdo-dist` goes to the extensions' Backlogs (`PM`,
`IAM`), implemented on `Loulen/pdo-extensions`. A seam the extension API lacks is a `CORE` Tâche:
the extension ticket is blocked by it (`tache block PM-17 --by CORE-1`) until the core tag that
ships it is pinned in `pdo-extensions`.
