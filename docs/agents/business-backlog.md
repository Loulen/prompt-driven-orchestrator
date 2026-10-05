# Business backlog

The **business** backlog carries the project's **user stories**. It is managed by humans
(PO / team); agents **read** it to start a design session and **post back** a traceability link
once the technical tickets are created. It is the upstream source of the pipeline ("Understand
the need").

> Distinct from the **technical** backlog (`docs/agents/issue-tracker.md`), which is by and for
> agents. N business items ≠ N technical tickets: one story can yield several tickets, or several
> items can collapse into one story.

## Tool

Tool: **PDO's own Project extension (`pm`)**, through its CLI `pdo ext pm` (the `pm` skill holds
the full command set). Business and technical items share the same Backlog; the Property `type`
tells them apart (`metier` vs `tech`, see `docs/agents/triage-labels.md`).

- **Instance**: the `pdo-dist` daemon (port 5272). The CLI that talks to it is the Distribution's
  `pdo` (`$(brew --prefix pdo-dist)/bin/pdo`). The `pdo` v1 has no `ext` subcommand: if
  `pdo ext pm help` answers `unrecognized subcommand 'ext'`, call the Distribution's binary by
  its full path.
- **Backlog**: **`CORE`** (name `core`), for this repo `Loulen/PDO-v2`. The extensions' Backlogs
  (`PM`, `IAM`) belong to `Loulen/pdo-extensions` (see its own `docs/agents/`).
- **GitHub Issues**: the core's issues still open on GitHub stay there until they are closed; no
  new item goes to GitHub.

## Agent queue (story selection)

The maintainer names the item(s) in the session prompt (`CORE-8`, or « the backlog »). The agent
never picks one on its own.

## Taking a batch: one story per batch

The maintainer drops raw items into the Backlog (`type=metier`, `To do`) and later takes a
**batch** of them. Grilling a batch:

1. creates **one** story (`type=metier`) that consolidates the batch, in the layout of
   `docs/agents/us-format.md`, with **one section per item** whose heading keeps the item's title
   and its old ID (« (ex CORE-3) »);
2. sets that story to `In progress`;
3. moves the original items to the Corbeille (restorable), so the Backlog only shows the story.

A deleted Tâche loses its ID (unlike a moved one): the « ex CORE-n » mention is the only trace.

## Commands / gestures

```bash
pdo ext pm tache list --backlog CORE --status "To do"              # LIST the items
pdo ext pm tache show CORE-8 ; pdo ext pm commentaire list CORE-8  # READ a story and its thread
pdo ext pm tache create "<titre>" --backlog CORE --set "type=metier" --status "In progress" \
  --body-file us.md                                                # CREATE the batch story
pdo ext pm tache delete CORE-3                                     # to the Corbeille (batch taken)
pdo ext pm tache comment CORE-8 - <<'EOF'                          # COMMENT (traceability)
…
EOF
pdo ext pm tache update CORE-8 --set "tasks=CORE-9,CORE-10"        # link the technical tickets
```

## Link to the technical backlog

The story's Relation **`tasks`** lists its technical tickets, and `/to-tickets` comments their
references on the story. The **spec** is a **sub-page of the story** (`page create --parent CORE-8`).

    story (type=metier, In progress)
      ├─ /grill-with-docs → CONTEXT.md + ADRs   (on integration/<story-ID>-<slug>)
      ├─ /to-spec         → the spec, a sub-page of the story
      └─ /to-tickets      → technical Tâches (type=tech), in the story's Relation `tasks`
          └─ implementation (see git-flow); the story is Done when integration → main is merged
