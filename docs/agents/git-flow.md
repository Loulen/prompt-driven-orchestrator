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
- **Version discipline (semver).** On a merge, reconcile `Cargo.toml` / `Cargo.lock` /
  `CHANGELOG.md`: a `feat` is a minor bump, a `fix` a patch, a breaking change (`!`) a major. The
  CHANGELOG records only breaking changes + notes that don't follow from a commit title.
- **Merged `integration/*` branches are throwaway** — delete once merged (skill's *Cleanup*); never
  pile new work onto an already-merged branch.

> Grows past solo? Revert to the skill's full flow (add `develop`, gate integration→main on HP + a
> human merge) and shrink or delete this file.
