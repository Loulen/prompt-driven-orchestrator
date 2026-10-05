# Triage Labels

The skills speak in terms of five canonical triage roles. This repo has **no labels**: items live
in `pm`, Backlog `CORE` (`docs/agents/business-backlog.md`), and the Property **`type`**
(Multi-select) carries their kind:

- `metier` — a business user story (the business backlog).
- `tech` — a technical ticket (the technical backlog, `docs/agents/issue-tracker.md`). A spec is
  not a Tâche: it is a sub-page of its story.

| Canonical role    | In `pm`                           | Meaning                                              |
| ----------------- | --------------------------------- | ---------------------------------------------------- |
| `needs-triage`    | _(a Commentaire)_                 | Say it in a comment; the maintainer evaluates        |
| `needs-info`      | _(a Commentaire)_                 | Ask in a comment                                     |
| `ready-for-agent` | `type=tech`, Status `To do`       | Ticket fully specified, agent-grabbable              |
| `ready-for-human` | _(a Commentaire)_                 | Say it in a comment                                  |
| `wontfix`         | a Commentaire, then the Corbeille | `pdo ext pm tache delete <ID>` after saying why      |

When a skill says « apply the `ready-for-agent` label », create the Tâche with `--set "type=tech"`
and `--status "To do"`. A role with no mapping is never created as a Property option: write it in
a comment instead.
