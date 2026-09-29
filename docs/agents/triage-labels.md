# Triage Labels

The skills speak in terms of five canonical triage roles. This repo carries **only two labels**,
both **type** labels:

- `US` — a business user story (the business backlog, see `docs/agents/business-backlog.md`).
- `tâche` — a technical ticket or a spec/PRD (the technical backlog, see
  `docs/agents/issue-tracker.md`).

| Canonical role    | Label in our tracker | Meaning                                              |
| ----------------- | -------------------- | ---------------------------------------------------- |
| `needs-triage`    | _(no label)_         | Say it in a comment; the maintainer evaluates        |
| `needs-info`      | _(no label)_         | Ask in a comment                                     |
| `ready-for-agent` | `tâche`              | Spec/PRD or ticket, fully specified, agent-grabbable |
| `ready-for-human` | _(no label)_         | Say it in a comment                                  |
| `wontfix`         | _(no label)_         | Close the issue as "not planned", with a comment     |

When a skill mentions a role (e.g. "apply the `ready-for-agent` triage label"), use the label in
this table. A role with no label is never created as a label: write it in a comment instead.
