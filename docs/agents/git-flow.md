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
- **Sanity gate before `main` moves:** `make check` (cargo check + frontend typecheck) must be
  green. `make test` and HP are on request only.
- **A merge closes no Tâche by itself** (tickets live in `pm`, Backlog `CORE`, no `Closes #N`):
  the node that validates a ticket sets it to `Done` once its merge is **on the remote**; the story
  goes to `Done` at the `integration/*` → `main` merge. Branches are named after the story's ID
  (`integration/CORE-8-<slug>`) and the ticket's ID (`feature/CORE-9-<slug>`).
- **No version bump on a ticket.** A ticket touches no version (workspace, extension API, the
  front's shared module, `Cargo.toml` / `Cargo.lock`) and takes no ADR number
  (ADR draft-version-changelog-et-numero-d-adr-se-fixent-a-la-fusion-vers-main).
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
