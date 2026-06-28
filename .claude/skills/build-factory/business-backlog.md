# Business backlog

The **business** backlog carries the project's **user stories**. It is managed by humans
(PO / team); agents **read** it to start a design session and **post back** a traceability link
once the technical issues are created. It is the upstream source of the pipeline ("Understand
the need").

> Distinct from the **technical backlog** (`docs/agents/issue-tracker.md`), which is by and for
> agents. N business issues ≠ N technical issues: one story can yield several technical issues,
> or several stories can collapse into one.

## Tool

Tool: `<tool to define>` (Jira, Trello, Notion, Linear, GitHub/GitLab Projects, spreadsheet,
markdown…). Fill in below how to reach it. If no tool is integrable (no CLI/MCP), the flow still
holds: the back-link (step 6 of `/to-issues`) becomes a manual gesture.

- **Access / tool**: `<CLI, MCP, URL, or "manual">`
- **Location**: `<board / project / page — id or URL>`
- **Auth**: `<how to authenticate, or "already configured">`

## Agent queue (story selection)

The agent does **not** pick at random: a human **selects** the stories to work on upstream,
typically by moving them into a dedicated column/state. The agent only picks from there.

- **Where the agent picks**: `<column / state / label — e.g. "To do", "Ready">`
- **Transition at start**: `<state → state when design starts — e.g. "To do" → "Doing">`
  (trigger when: grilling done + story selected + technical counterpart created)
- **Transition to review**: `<state → state when the PR opens — e.g. "Doing" → "Test">`

## Commands / gestures

```
# READ a story:        <command or gesture>
# COMMENT on a story:   <command or gesture>   (used by /to-issues for traceability)
# MOVE / change state:  <command or gesture>
```

## Link to the technical backlog

Keep the **business reference** (URL or story id) in the PRD / technical issues, for two-way
traceability.

    business backlog (story selected by a human)
      └─ /grill-with-docs   → CONTEXT.md + ADRs   (on integration/<business-ref>-<slug>)
          └─ /to-prd        → PRD on the technical backlog
              └─ /to-issues → technical issues + back-comment on the business story
                  └─ implementation (see git-flow)
