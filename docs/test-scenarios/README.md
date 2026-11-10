# Agentic tests — Happy Path inventory

The **agentic test** layer (apex of the testing pyramid, ADR-0004): a subagent drives the **real
running app** and validates a user journey, UI-first, raising **findings** rather than a binary
verdict.

- **Runner:** the `/agentic-tests` skill (mode selection + gates).
- **Format:** [`SCENARIO-FORMAT.md`](../../.claude/skills/agentic-tests/SCENARIO-FORMAT.md).
- **Driving PDO:** [`docs/agents/run-scenario.md`](../agents/run-scenario.md) — how an agent drives
  the app and probes side-effects (browser, `tmux capture-pane`, daemon HTTP, filesystem).

Two levels:

- **Happy Path (`HP-`)** — curated, **permanent** suite (the paths taken by 80 %+ of users). Worth
  as much as critical-path documentation as it is a regression suite. Lives here. Gate
  `integration → main`. **At most 3.** Curation is a human decision (see `git-flow`).
- **Feature Path (`FP-`)** — **no file here.** It lives in the "Acceptance criteria → Feature Path"
  section of a technical-backlog ticket and is **throwaway** (it dies with the ticket). If a piece
  of an FP is worth keeping, graft it **drive-by** onto an HP.

> **Resilience / adversity is not a Happy Path.** It is edge-case robustness, expensive to drive,
> and covered **permanently by the layer-3 automated tests** (`tests/process_lifecycle.rs`,
> `crates/pdo/tests/`, `frontend/e2e/`). It does not earn an HP slot.

## The core alone (vanilla)

The suite is played on the **vanilla core**: the binary this repository builds, with no extension
loaded — `make demo-daemon PORT=<port>` without `TEST_EXT` (see the driving playbook). That is the
product published as **PDO**; a Distribution (Acogni Driver: the core plus private extensions)
plays the same journeys on top of its own extensions, never instead of them. So:

- no journey step, precondition or check needs an extension (no Backlog, no Rights, no extension
  tab or route); a check that only an extension can satisfy says so and is skipped on the core;
- the **product's name and logo** come from the binary's **Marque** (CONTEXT.md): « PDO » and its
  mark on the core, the Distribution's own elsewhere. One step checks them, HP-03's step 16
  (Settings › General › Version & update reads `PDO <version>` after the PDO mark, signed « by
  Acogni »); a Distribution reads its own there. No other step depends on the brand.

Start the demo daemon with **`BIND=0.0.0.0`** (or `BIND_ALL=1`) for HP-02: its sandboxed Run calls the daemon from a
container, which a loopback-only daemon refuses. Every harness HP-02 runs must be logged in on the
machine (the demo daemon copies the logins it finds, and skips the others): a missing login is a
precondition finding, not a product bug.

## Inventory

| ID | Title | Covers | Status |
|---|---|---|---|
| [HP-01](HP-01-author-and-save.md) | Author & save a pipeline | Pipelines tab, unified canvas, shared draft, Save window and pipeline versions, duplicate, Validate | active |
| [HP-02](HP-02-run-to-completion.md) | Launch a run to completion | run lifecycle, dataflow, artifacts, stats, Approve, **diff review loop (comment → manager reply → resolve)**, **sandbox A/B (`full` vs `off`)**, **harness four-way choice (`claude` / `opencode` / `copilot` / `pi`)**, **live session on two browsers (watch, take control, import, a declared wait a bare Enter never lifts)** | active |
| [HP-03](HP-03-guided-onboarding.md) | A newcomer takes the full tour | welcome modal, tutorial mode, Projecteur, training repository, *Overview* on a script Run + example Trigger tidy-up, New Run form, interactive node + Approve ▾, artifact, *First pipeline* + Validate, step replay | active |

**The 3rd slot went to onboarding (#825, spec #821), not to Triggers.** The full tour is the only
journey that is *somebody's first five minutes*: it crosses the welcome modal, the New Run form, a
real Run on a real agent, the completion guard and the artifact panel, and it breaks **silently** —
a tour whose target moved does not fail a build, it strands a newcomer on a dimmed screen. It also
costs almost nothing beyond what HP-02 already spends, since the Run it launches is one small node,
and *Overview* already shows a Trigger. The day Triggers deserve a journey of their own, this slot is
the one to argue with (merge two journeys, drop a non-critical one, or graft drive-by) — the limit
of **3** stands, and curation is a human decision at the integration→main MR.

HP-03 was promoted from `proposed` to `active` with CORE-227; its replay on the vanilla core, with
HP-01 and HP-02, is reported in that ticket.

**The diff review loop is grafted onto HP-02 (its §17) rather than given its own slot.** It reuses the
Run HP-02 already completed, costs one manager turn, and is the only place a real agent answers
through the Review surface: the acceptance tour of the Review and observation domain found its first use
broken there, silently, when no step of any HP crossed it.

**Sandbox (PRD #403) is grafted onto HP-02 as an A/B drive-by rather than given its own slot.** The
pair `full` + `off` is what makes it meaningful: the `off` twin is the control (a Run that silently
fell back to the host path also looks green), and `full` — not `minimal` — is what makes the
"no node before the sandbox is ready" guarantee testable at all. Two consequences worth knowing
before running the suite: each execution copies ~1 GB of staging and spends tens of seconds
preparing, and the journey must **stay on the Run** during preparation (see HP-02's notes for why
that is load-bearing).

**The agentic-harness axis (PRD #549, #612, #702) is grafted onto HP-02 for the same reason, at a
far lower price.** A single-harness Run that silently resolved to the `claude` floor is
indistinguishable from a correctly resolved one, so the other harnesses are the control that makes
the resolution observable. It costs four panes inside one Run, no staging, no image — which is why
it rides HP-02's `off` side rather than claiming a slot. The four harnesses are the whole spread of
PDO's instrumentation, exactly what the **support table** of `docs/reference/harnesses.md`
publishes: HP-02 is the one place the published table and the running product are compared.
