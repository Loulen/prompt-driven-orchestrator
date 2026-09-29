# Business User Story — layout

The lean, tech-agnostic layout for a business **User Story**: a title plus seven body blocks. A
project may tailor it freely — add blocks a domain needs, drop ones it doesn't.

**Altitude — value and behavior.** A User Story speaks the "what" and the "why". No field names, no
technology, no data model: that is decided later in the per-story design grilling (`/grill-with-docs`
→ `/to-spec` → `/to-tickets`).

## Title

`<Feature> US-<n> [user|admin|tech] <Verb (infinitive) + object>`

- `user` — value visible to the end user · `admin` — an operational gesture (ingestion, reporting) ·
  `tech` — foundation/tooling with no direct business-observable behavior.

Example: `Perimeter US-1 [admin] Register the site perimeter by file upload`

## Body

```markdown
**INTENTION**

As a <user | admin | precise role>, I want <capability>, **so that** <the benefit — not the
mechanism; this is what justifies the story>.

**CONTEXT**

<What a cold reader must know: where the value chain stands, what already exists, what is
deliberately reduced to a prototype, and the observable if there is no screen.>

**EXPECTED BEHAVIOR**

- <Nominal flow: action → visible effect.>
- <Non-conforming case: motivated rejection, nothing is written.>

**BUSINESS RULES**

1. <Numbered rule, referenceable by its number from the other blocks.>
2. <…>

**ACCEPTANCE CRITERIA**

- <Action 1> → <expected observable result 1>
- <Action 2> → <expected observable result 2>

**OUT OF SCOPE**

- <What this story deliberately does not do — with the pointer: deferred, sibling US, later target.>

**DEPENDENCIES**

- <Upstream (blocking) stories, sibling features, data or computations required.>
```

## Quality checks

- **Intention** — the "so that" carries the value; if it's hollow, the story was born wrong.
- **Behavior vs Rules** — behavior tells the *flow*; rules settle the *decisions* (uniqueness,
  tolerance, rejections). A rule cited by the behavior is referenced by its number.
- **Acceptance criteria** — concrete and observable, so they translate cleanly into the per-ticket
  **Feature Path** downstream: a concrete action, an observable result.
- **Out of scope and Dependencies** mirror each other — what the story may not do vs what it needs to
  exist.

## Tailoring

A project tailors this layout in its `docs/agents/us-format.md` copy — for example adding blocks a
domain needs: data tables touched, a design/mockup link, or test datasets. Keep additions at value
altitude; anything technical belongs downstream, not in the User Story.
