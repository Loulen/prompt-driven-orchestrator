# Git flow (this repo)

Project overrides to the `git-flow` skill. **Solo dev — the flow is deliberately relaxed.** Where
this file and the skill disagree, **this file wins**; the skill describes the full team flow.

## What differs from the skill

- **No `develop` branch.** `main` is the only integration target (`origin/HEAD → main`).
- **`integration/<business-ref>-<slug>` merges straight to `main`** — no develop hop.
- **The HP suite is opt-in.** The skill runs `/agentic-tests HP` before an integration→main merge;
  here it runs only on request.
- **The agent may perform the merge.** The skill reserves develop/main merges for a human; solo,
  the agent merges (and reconciles conflicts) directly.
- **`main` is not branch-protected** — direct push / fast-forward is allowed, so a merge is usually
  `merge into the integration branch → fast-forward main`, no PR required.

## What still holds

- **Branch shape** stays the skill's: `integration/*`, `feature/*`, `hotfix/*`, off the right base.
- **Three gates** (ADR-0094):
  - `make fast` while you work: `cargo fmt --check` and the structural checks of `crates/tidy`, in
    under a minute even on a fresh checkout.
  - `make scoped` after a change: `make fast`, then only what the diff since the merge-base with the
    branch you started from touches (`BASE=` to change it). It prints its plan first: frontend ->
    typecheck, lint, vitest; a crate -> nextest and clippy on it and its dependents; a type
    exported to the front -> `types-check`; doc only -> nothing more.
  - `make gate`, the **gate of a ticket**, once, after the FP and its minor fixes (those go
    through `make scoped` on the delta, replaying only the FP steps they touch): every step
    `.github/workflows/ci.yml` declares except the E2E, without fail-fast, plus vitest and the
    script tests. It lists every failure at the end.

  A ticket merges into its `integration/*` on a green `make gate`, a green FP and no blocking
  finding. The E2E (`make e2e`) runs once per story, before `integration/*` -> `main`. HP is on
  request only.
- **A merge closes no Tâche by itself** (tickets live in `pm`, Backlog `CORE`, no `Closes #N`):
  the node that validates a ticket sets it to `Done` once its merge is **on the remote**; the story
  goes to `Done` at the `integration/*` → `main` merge. Branches are named after the story's ID
  (`integration/CORE-8-<slug>`) and the ticket's ID (`feature/CORE-9-<slug>`).
- **No version bump on a ticket.** A ticket touches no version (workspace, extension API, the
  front's shared module, `Cargo.toml` / `Cargo.lock`) and takes no ADR number
  (ADR-0095).
  It writes its CHANGELOG entry under `## Unreleased`, the first entry, in English and without
  ticket ids: what changes on screen, the new seams, the breaking changes. A new ADR is born
  `docs/adr/draft-<slug>.md`. The binary embeds the CHANGELOG for "What's new", which hides
  `Unreleased`. `.gitattributes` merges `CHANGELOG.md` with the `union` driver, so two tickets
  adding a bullet at the same place merge without a conflict: both lines are kept.
- **Merged `integration/*` branches are throwaway** — delete once merged (skill's *Cleanup*); never
  pile new work onto an already-merged branch.

## `integration/*` -> `main`

The version, the CHANGELOG and the ADR numbers are set here, once per story:

1. Bring the integration branch up to date with `main`, then `make e2e`: the frontend's Playwright
   suite, which no ticket runs. It must be green.
2. `make release-bump DRY_RUN=1` prints the semver level the conventional commits since the last
   tag propose (`feat` a minor, `fix` a patch, `!` or `BREAKING CHANGE:` a major, any other commit a
   patch) and the plan. The agent **confirms the level** (with the maintainer when the commits
   do not tell a break the story made), then runs `make release-bump`, with `LEVEL=` to force
   another one (`API_LEVEL=` for the extension API).
3. The command bumps the workspace, the extension API, the front's shared module and
   `Cargo.lock`, turns `## Unreleased` into the version's entry and numbers the draft ADRs,
   rewriting their citations. Check the diff, commit `chore(release): vX.Y.Z` on the integration
   branch, then merge into `main`. A version bump pushed to `main` is a release (tag on version).

> Grows past solo? Revert to the skill's full flow (add `develop`, gate integration→main on HP + a
> human merge) and shrink or delete this file.
