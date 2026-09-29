# Business backlog

The **business** backlog carries the project's **user stories**. It is managed by humans
(PO / team); agents **read** it to start a design session and **post back** a traceability link
once the technical issues are created. It is the upstream source of the pipeline ("Understand
the need").

> Distinct from the **technical backlog** (`docs/agents/issue-tracker.md`), which is by and for
> agents. N business issues ≠ N technical issues: one story can yield several technical issues,
> or several stories can collapse into one.

## Tool

Tool: **GitHub Issues, label `US`**, in the same repo as the technical backlog. Business stories
and technical tickets share the tracker; the label tells them apart (`US` vs `tâche`, see
`docs/agents/triage-labels.md`).

- **Access / tool**: `gh` CLI
- **Location**: this repo's issues, `--label US`
- **Auth**: `gh auth status`

## Agent queue (story selection)

The maintainer names the story (its issue number) in the session prompt. The agent never picks one
on its own.

## Commands / gestures

```
# READ a story:        gh issue view <number> --comments
# LIST the stories:    gh issue list --label US --state open
# COMMENT on a story:  gh issue comment <number> --body "..."   (used by /to-tickets for traceability)
# CREATE a story:      gh issue create --label US --title "..." --body "..."   (used by /to-us)
```

## Link to the technical backlog

Keep the **business reference** (the `US` issue number) in the spec /
technical tickets, for two-way traceability.

    `US` issue (named by the maintainer)
      └─ /grill-with-docs    → CONTEXT.md + ADRs   (on integration/<business-ref>-<slug>)
          └─ /to-spec        → spec on the technical backlog
              └─ /to-tickets → technical tickets
                  └─ implementation (see git-flow)
