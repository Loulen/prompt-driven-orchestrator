---
name: verify-factory
description: Re-runnable health check for the factory wiring in this repo. Probes both backlogs, triage labels, tooling auth, the agentic driver, the build-factory scaffold and context, that the factory artifacts are versioned (not gitignored), git-flow health, and the US template; reports pass/fail, offers to fix or create what's missing, and signposts the next step. Guided, not gated — it never blocks. Run it anytime to see where the factory stands.
disable-model-invocation: true
---

# /verify-factory

A **health check** for the factory. It looks at what `/build-factory` wired and what the pipeline
skills expect, tells you what's green and what's missing, and offers to fix each gap in place.
Re-run it anytime — after `/build-factory`, after changing a backlog, or when a skill behaves as if
something isn't wired.

**Guided, not gated (ADR-0001).** Every check reports and offers; none of them blocks. The compliant
path is made the obvious path, never the only one. `/verify-factory` writes nothing on its own — it
proposes, you approve, it acts.

## How each check runs

Every check below has the **same shape**, so read the shape once and apply it to all of them:

1. **Probe** — read the relevant port under `docs/agents/*.md` (the project-tailored wiring), then
   run a real, read-only probe against the tool it names. Assume nothing; a port that is still a
   `<tool to define>` stub is itself a finding.
2. **Report** — `✅ pass` / `❌ fail` / `➖ n/a`, one line, with the concrete evidence (the command
   output or the missing file).
3. **Offer** — for a fail, name the smallest fix and offer to do it now (create the label, the
   branch, the file…). Wait for a go-ahead before any write.
4. **Signpost** — say which skill owns this piece, so the person knows where a deeper fix lives.

Run the checks in order, then close with the **summary** (§11). Collect findings as you go; don't
stop on the first fail — the point is the whole picture.

## 1. Scaffold present (build-factory scaffold output)

Per ADR-0004, `/build-factory` has two outputs; this is the first. Probe that the **scaffold** is in
place:

- `CLAUDE.md` (or `AGENTS.md`) carries the **method constants** block and an `## Agent skills` block.
- `docs/agents/{business-backlog,issue-tracker,triage-labels,domain}.md` all exist.
- `docs/adr/` exists and a root `CONTEXT.md` exists (even as a stub).

Missing scaffold → **signpost `/build-factory`** (scaffold mode). Offer to list exactly which of the
files above are absent.

## 2. Factory artifacts are versioned (not gitignored)

The factory *lives in the repo*: `CONTEXT.md`, the ADRs, `docs/agents/` and the vendored skills
(canonically under `.agents/skills/`, with `.claude/skills` a symlink overlay for Claude) are
**code — committed and shared with the team**, not local state. If `.gitignore` swallows any of
them, a **fresh clone gets a silently broken factory** — no skills, no context, and nothing warns
anyone. This trap appears *after* setup too (an editor, a template, or an upstream's `init
CLAUDE.md`), so it earns its own recurring check. Probe it:

- **Nothing factory-owned is ignored** — `git check-ignore -v .agents/skills .claude/skills
  CONTEXT.md docs docs/adr` (add any path the install actually used). **No output → `✅ pass`.** Each
  line printed is a factory artifact that an active `.gitignore` rule hides, and names the offending
  rule → `❌ fail`, listing the swallowed artifacts and the rules behind them. A blanket `.claude`
  (or `.agents`) is the usual culprit — editors, templates and some upstreams add it to drop *local*
  agent state (e.g. Excalidraw gitignores `.claude` since its "init CLAUDE.md").

On a fail, **offer to narrow** the rule rather than delete it wholesale: keep genuinely-local files
ignored (replace a blanket `.claude` with `.claude/settings.local.json`, or add a negation
`!.agents/skills/`) so skills + context get versioned while local settings stay out — then `git add`
the now-tracked artifacts, on a go-ahead. Never widen the ignore. Signpost the `git-flow` skill (the
repo's version-control conventions) and `/build-factory` (the scaffold these paths come from).

## 3. Context filled (build-factory context output)

The second `/build-factory` output (ADR-0004). Probe that the **context** is bootstrapped, not just
scaffolded:

- `CONTEXT.md` holds a real glossary — resolved terms under its section headers, not just the empty
  stub headers.
- At least the foundational **ADRs** exist under `docs/adr/` (more than an empty directory).

Empty `CONTEXT.md` or no ADRs → **signpost `/build-factory` context mode** (the project-context
bootstrap grilling). The grilling sessions are the only writers of `CONTEXT.md`/ADRs, so
`/verify-factory` never fills them itself — it points at the skill that does.

## 4. Technical backlog — list and create

Read `docs/agents/issue-tracker.md` for the backend (GitHub `gh` / GitLab `glab` / local markdown /
other), then:

- **List** — run the port's list gesture (`gh issue list`, `glab issue list -F json`, or read
  `.scratch/`). A non-error response = the agent can read the backlog.
- **Create** — confirm write access without leaving junk behind. For a remote backend, `gh auth
  status` / `glab auth status` shows whether the token can write; for local, confirm `.scratch/` is
  writable. Offer an optional **round-trip smoke test** — create a ticket titled `verify-factory
  probe`, then close/delete it — **only** on an explicit go-ahead.

Fail → **signpost `/build-factory`** (technical backlog decision) to rewire the port.

## 5. Business backlog reachable

Read `docs/agents/business-backlog.md` for the tool (Jira, Trello, Notion, Linear, Projects,
spreadsheet, markdown…). Probe its **read** gesture / auth as the port documents it (CLI or MCP). A
port still set to `<tool to define>` or marked manual is **not a fail** — report it as an expected
**manual back-link**, and note that `/to-us` and `/to-tickets` degrade to a manual gesture there.

Fail → **signpost `/build-factory`** (business backlog decision).

## 6. Triage labels present

Read `docs/agents/triage-labels.md` for the canonical-role → tracker-string mapping, then check the
tracker actually carries each mapped string:

- GitHub — `gh label list`; GitLab — `glab label list`.
- Diff the mapped strings against what exists; report which of the five roles are **missing**.
- **Offer to create** each missing one (`gh label create "<string>"` / `glab label create
  --name "<string>"`), on a go-ahead.

Local-markdown backend → labels are `Status:` strings inside files, so there are no label objects to
create: **`➖ n/a`, pass by convention**. Signpost the `triage` skill for the role semantics.

## 7. Tooling auth

Probe the auth of **only the tools the ports actually name** (don't demand tools this project never
chose):

- `gh` → `gh auth status` · `glab` → `glab auth status` · `trello` → the port's whoami/list gesture ·
  `jira` → the configured CLI or MCP reachability check.

Report each as `✅ authenticated` / `❌ unauthenticated`. Fail → name the sign-in command from the
tool's own `--help` and offer to walk through it. Never store credentials from here.

## 8. Agentic driver launches

The **agentic test** layer drives the real running system through its **primary surface** (see
`CONTEXT.md`). Infer the app type from the repo, **recommend the matching driver**, and probe that it
launches — matching the `agentic-tests` skill:

| App type (primary surface) | Recommended driver | Launch probe |
|---|---|---|
| Web app (UI) | Playwright CLI (the CLI, not the MCP) | driver installed + a headless launch |
| Infra / cloud (cloud CLI) | AWS CLI / `gcloud` | CLI present + authenticated |
| CLI / TUI app (terminal) | tmux | tmux present + a scratch session opens |
| Data pipeline (warehouse) | dbt / SQL client | tool present + connects |

Report whether the recommended driver **launches**. Missing → offer the install/auth step and point
at `agentic-tests` for how the driver is used. This is a recommendation, not a mandate — the driver
stays the project's choice.

## 9. Git-flow health

Probe the two invariants `git-flow` relies on:

- **`develop` exists** — `git show-ref --verify --quiet refs/heads/develop` (or
  `git ls-remote --heads origin develop`). Missing → offer `git branch develop` from up-to-date
  `main`/current, so every `integration/*` branch has a base to fork from.
- **`main` protected** — GitHub: `gh api repos/{owner}/{repo}/branches/main/protection` (200 =
  protected); GitLab: `glab api projects/:id/protected_branches`. Unprotected → report it and offer
  the enable command/link.

Protection is **advisory** in PDSF (ADR-0001): report an unprotected `main`, offer to fix it, never
force it. Signpost the `git-flow` skill.

## 10. US template — default or tailored

Two probes, both about `docs/agents/us-format.md` (the update-safe project copy that `/build-factory`
seeds and that `/to-us` reads):

- **State** — absent → not seeded yet (**signpost `/build-factory`**). Present and byte-identical to
  the core default `skills/to-us/US-FORMAT.md` → **default (untailored)**. Present and diverged →
  **tailored** (the project shaped its own US layout — expected, report it as healthy).
- **Layout drift** — sample a handful of existing User Stories from the business backlog (read them
  via the §5 port). If they **consistently** follow a layout that differs from
  `docs/agents/us-format.md`, **ask the user whether to adapt `docs/agents/us-format.md`** to match
  what the team actually writes. This US-layout inference belongs here — `/build-factory`'s first
  runs never infer it. On a yes, propose the edited `docs/agents/us-format.md` and let the user
  approve before writing.

## 11. Summary and next-step signpost

Close with the whole picture:

- A **tally** — one line per check (`✅`/`❌`/`➖`), so the state is legible at a glance.
- The **single most useful next step**, chosen from the repo state (guided, not gated):
  - scaffold missing → `/build-factory` (scaffold mode).
  - scaffold present, `CONTEXT.md` empty → `/build-factory` context mode (the bootstrap grilling).
  - all wiring green → the pipeline is ready: `/to-us` to shape a feature into a User Story, or
    `/grill-with-docs` to design a picked story.

End on the next command, never on a gate. Re-running `/verify-factory` after any fix re-checks
everything from scratch.
