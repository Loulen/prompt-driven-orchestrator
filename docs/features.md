# PDO features

What each feature of PDO does, in more depth than the [README](../README.md) table. Commands,
settings and the harness support matrix live in [docs/reference/](reference/). The domain
vocabulary lives in [CONTEXT.md](../CONTEXT.md).

## Contents

- [Visual pipelines](#visual-pipelines)
- [Conditional routing & loops](#conditional-routing--loops)
- [Typed outputs](#typed-outputs)
- [Diff review](#diff-review)
- [Triggers](#triggers)
- [Run stats by model](#run-stats-by-model)
- [Interactive and orchestrator nodes](#interactive-and-orchestrator-nodes)
- [Agent profiles](#agent-profiles)
- [Skill bank](#skill-bank)
- [Also in the box](#also-in-the-box)
  - [Isolated worktrees](#isolated-worktrees)
  - [Live sessions](#live-sessions)
  - [Sandbox](#sandbox)
  - [Multi-repo runs](#multi-repo-runs)
  - [Guided tours](#guided-tours)
  - [Page mounts](#page-mounts)
  - [Service & in-app update](#service--in-app-update)
- [How PDO works](#how-pdo-works)

## Visual pipelines

A **pipeline** is a named graph of roles. You build it on a canvas: drop a node (`implementer`,
`reviewer`, a `script`), drag an edge from one of its output ports to the next node, and save. An
`agent` node runs a harness with the role's system prompt; a `script` node runs your bash, with no
LLM involved.

Underneath, a pipeline is plain YAML plus one prompt file per node, so it diffs, reviews and
travels like code (export and import carry its skills along). You can edit the graph while a run is
going: nodes that already run stay as they are, and the scheduler picks up the new graph on its next
tick.

Decisions: [ADR-0003](adr/0003-stack-rust-react-xyflow.md),
[ADR-0007](adr/0007-edit-during-run.md),
[ADR-0017](adr/0017-script-node.md),
[ADR-0059](adr/0059-les-pipelines-appartiennent-a-l-instance-et-voyagent-par-document.md).

## Conditional routing & loops

Routing lives on the edge. An edge can carry a `when:` clause that reads the frontmatter of the
artifact leaving its source port, a pipeline variable, or the loop counter `iter`. Predicates are
mechanical: `eq`, `neq`, `lt`, `lte`, `gt`, `gte`, `in`, `not_in`. An `else` edge fires only when no
sibling edge matched. There is no LLM router: an LLM never decides which node runs next.

Drag an edge back from `reviewer` to `implementer`, set `verdict = fail`, and PDO turns the cycle
into a named, **bounded loop** (`↻ 1/8` on the canvas) with a `max_iter`, so a cycle can never run
forever. A **collection loop** fans a node out over a list field of its input, with laps running in
parallel.

Decisions: [ADR-0002](adr/0002-mechanical-conditionals-only.md),
[ADR-0011](adr/0011-conditional-edges-and-loop-regions.md),
[ADR-0026](adr/0026-collection-region-live-wiring.md).

## Typed outputs

Each node declares its output ports: what it hands over. A port can be markdown with a frontmatter
schema (the `verdict` a routing edge reads), an `image_list` (annotated screenshots, shown as
thumbnails and opened in a lightbox), `html` (rendered in a sandboxed frame), or files. Markdown
renders with its Mermaid diagrams.

PDO validates each output when the node completes. A missing file or a frontmatter field that breaks
the schema refuses the completion, and the agent is told why in its own session, before the next node
starts. Artifacts live in the run's blackboard and survive archiving.

Decisions: [ADR-0010](adr/0010-port-output-as-directory.md),
[ADR-0013](adr/0013-client-side-mermaid-rendering.md),
[ADR-0020](adr/0020-archive-preserves-outputs.md),
[ADR-0028](adr/0028-html-output-port.md),
[ADR-0035](adr/0035-completion-refusal-is-never-2xx.md).

## Diff review

The run's **Diff** tab shows everything the run changed since it forked. **Expand and comment**
opens the review page: a diff between any two refs of the run (fork point, run tip, each node's
before/after, the live working tree), side by side, with inline comments on any line.

**Send to manager** hands your comments to the run's manager agent in one message. It answers each
one with `pdo review reply`, the answer lands in the thread in real time, and the manager can route
the fix back into the pipeline (restart a node, inject an artifact). By default you resolve a
comment; the agent only proposes the resolution.

Decisions: [ADR-0067](adr/0067-la-relecture-de-diff-est-une-conversation-dans-l-event-log-entre-deux-refs-du-run.md).
Commands: [`pdo review`](reference/cli.md#cli-commands).

## Triggers

A **trigger** starts a run of a pipeline on a cron (five fields, UTC), optionally behind a **guard
script**. The guard runs first, before anything is spawned: exit 0 fires, any other exit skips the
tick, and **the guard's stdout becomes the run's input**. With `* * * * *` and
`./prod-health-check.sh`, a guard that exits 0 only when production is failing turns an outage into
an incident run, with the incident report as input. **Test guard** dry-runs it from the trigger
panel, and the fire history lists every fired and skipped tick.

Other guards follow the same shape: "main broke last night" (the nightly CI run is red) or "red
Dependabot PRs" (the list of failing dependency PRs becomes the input).

One firing is one run, and a trigger skips its tick while its previous run is still alive. PDO keeps
no dedup state: the pipeline mutates what the guard polls (a label, an issue), and that is the dedup
register. A trigger only starts runs; whether the pipeline can push or merge is the pipeline's own
business.

Decisions: [ADR-0012](adr/0012-triggers-and-trust-earned-autonomy.md),
[ADR-0027](adr/0027-manual-trigger-fire-first-class.md).

## Run stats by model

The **Stats** page aggregates your own runs over a period: Overview (runs, errors, sessions), fires
per pipeline, Sessions, **Cost** and **Performance**. Cost breaks down by harness, by node, and **by
model then effort**, with the model the execution actually ran on, read from the harness's own
transcript. Performance shows duration, failure rate and steering messages per node or per model as
box plots. Every summary is a median, never a mean.

Cost is an estimate read from local transcripts: derived for `claude` (tokens × a price table fed
from models.dev, overridable by hand), reported for harnesses that count it themselves. A harness
that cannot tell says "—", never `$0`.

Decisions: [ADR-0022](adr/0022-estimated-cost-from-local-transcripts.md),
[ADR-0029](adr/0029-aggregated-stats-derive-and-index.md),
[ADR-0034](adr/0034-out-of-band-price-fetch.md),
[ADR-0052](adr/0052-un-cout-rapporte-se-convertit-par-une-constante-et-ne-passe-pas-par-la-table-de-prix.md),
[ADR-0065](adr/0065-modele-et-effort-observes-source-d-abord-intention-en-repli.md).
Which harness reports what: [harnesses.md](reference/harnesses.md).

## Interactive and orchestrator nodes

Two flags of an agent node, independent of its type (`agent`, `script`, `merge`): together they
make its **node kind**, and a node can be both.

Turn **Interactive** on and a human talks to the node. When its agent needs you (a question, a
choice, a review), it declares the wait with `pdo wait-user --message "<question>"`: the node and its
run turn **awaiting you**, with the question on the banner. You reply in the node's terminal, and
your Enter lifts the wait. PDO never infers a wait from a silence. The node's completion is guarded
until you release it (« Mark ready for completion ») or force it (« Mark complete »).

Turn **Orchestrator** on and its session learns to launch runs itself: `pdo run create` starts a
child pipeline (the same one, or another), so pipelines run pipelines, and `pdo run wait` blocks
until a child is done. The daemon records who launched what, so the run list becomes a **run
tree**: children fold under their parent, with counters for finished, failed, stale and running
descendants. The node's **Orchestration** tab follows its children to the end, and a child that
waits for you shows on its parent too.

Decisions: [ADR-0064](adr/0064-la-provenance-parent-enfant-est-mecanique-et-la-liaison-orchestrateur-est-forte.md),
[ADR-0068](adr/0068-la-liberation-de-la-completion-est-une-garde-du-daemon-pas-un-message-dans-le-pane.md),
[ADR-0069](adr/0069-l-attente-utilisateur-est-declaree-par-l-agent-et-le-suivi-des-enfants-est-un-pull-bloquant.md).
Commands: [`pdo wait-user`, `pdo run create`, `pdo run wait`](reference/cli.md#cli-commands).

## Agent profiles

An **agent profile** names a harness · model · effort combination once, in Settings. Wherever PDO
picks a harness (node, run, project, instance), you choose **Inherit**, a named profile, or
**Custom**. Change a profile and every node that follows it switches, with no pipeline to edit. The
reference stays live until the node starts, which then freezes its values for that execution.

The **Default** profile is the instance floor (initially `claude`). Before a profile is deleted, PDO
lists everything that still references it.

Decisions: [ADR-0046](adr/0046-le-harnais-est-un-axe-a-quatre-tiers-modele-et-effort-sont-conditionnes-par-lui.md),
[ADR-0057](adr/0057-les-profils-agentiques-restent-des-references-vivantes-jusqu-au-spawn.md).

## Skill bank

The **skill bank** holds the skills PDO gives to nodes. Import them from a repository, a subfolder
or a local folder (every folder with a `SKILL.md` is listed, and the ones you tick are filed under a
folder named after the source), or write one by hand with its reference files. Then pick skills for a
node, a run, a project or the whole instance: the effective set is the union of the four.

PDO copies the effective skills into every worktree it creates, where `claude`, `copilot` and
`opencode` read them natively, and keeps them out of version control.

Decisions: [ADR-0062](adr/0062-les-skills-sont-livres-dans-le-worktree-jamais-commites.md).

## Also in the box

### Isolated worktrees

Every run gets its own git worktree and branch, and two runs never share one. Each node picks where
it works: an `agent` node works in its own sub-worktree by default, merged back into the run's branch
when it completes, or it opts into the run's shared worktree (Workspace: isolated / shared).
Provisioning (files to copy, commands to run) is declared once and frozen per run.

Decisions: [ADR-0036](adr/0036-merge-back-resolved-in-the-node-favour.md),
[ADR-0060](adr/0060-l-isolation-est-un-choix-du-node.md),
[ADR-0061](adr/0061-le-provisionnement-de-worktree-est-declaratif-additif-et-gele-par-run.md).

### Live sessions

Every node runs in a tmux session you can watch from the web terminal, type into, or take over. An
interactive node waits for you, and an agent can declare that it waits on you with `pdo wait-user`.
A shell can be opened in any run's worktree. Copy and paste: [terminal.md](reference/terminal.md).

Decisions: [ADR-0005](adr/0005-inline-xterm-over-os-spawn.md),
[ADR-0021](adr/0021-run-shell-open-session.md).

### Sandbox

Run nodes inside a container profile. PDO stages the harness home (credentials, settings) into the
sandbox at spawn and harvests transcripts back, so cost and stats still work. The image is yours.

Decisions: [ADR-0030](adr/0030-sandbox-execution-model.md),
[ADR-0031](adr/0031-sandbox-staging-profiles.md),
[ADR-0063](adr/0063-le-staging-sandbox-est-un-staging-set-par-harnais-rempli-au-spawn-du-noeud.md).

### Multi-repo runs

One run can work across several repositories: a primary one it writes to, and secondary ones given
to every node as snapshots, writable by default or read-only per repository.

Decisions: [ADR-0042](adr/0042-multi-repo-is-a-per-run-axis-secondaries-are-read-only-snapshots.md),
[ADR-0047](adr/0047-secondary-repos-are-writable-by-default-read-only-is-a-per-repo-opt-in.md).

### Guided tours

The app walks you through your first pipeline one step at a time, on the real UI and on a throwaway
tutorial repository.

Decisions: [ADR-0071](adr/0071-un-tour-pilote-la-vraie-ui-et-observe-le-vrai-etat.md).

### Page mounts

`pdo page mount <name> <directory>` serves a directory as real HTML under `/pages/<name>/`: agents
show you prototypes and reports through the instance, without a rebuild.

Decisions: [ADR-0066](adr/0066-les-page-mounts-servent-du-html-navigable-sur-geste-explicite.md).

### Service & in-app update

`pdo service install` installs a systemd user unit or a launchd agent, so PDO starts at boot and
keeps running after logout. When a new release is out, one click on the version in the status bar
updates PDO with your install method and reloads the page. See [cli.md](reference/cli.md).

Decisions: [ADR-0019](adr/0019-persistent-daemon-service-unit.md).

## How PDO works

| Principle | Behavior | Reference |
| --- | --- | --- |
| Deterministic orchestration | Typed outputs and graph rules choose the next node | [ADR-0002](adr/0002-mechanical-conditionals-only.md), [ADR-0011](adr/0011-conditional-edges-and-loop-regions.md) |
| Typed artifacts | Each node emits validated frontmatter and a content body | [ADR-0020](adr/0020-archive-preserves-outputs.md) |
| Expert control | Interactive nodes wait for input and expose a live terminal | [ADR-0005](adr/0005-inline-xterm-over-os-spawn.md) |
| Deliberate autonomy | Only nodes placed in the pipeline can push, open PRs, or merge | [ADR-0012](adr/0012-triggers-and-trust-earned-autonomy.md) |
| Local agent setup | Sessions use your harness configuration and skills | [ADR-0045](adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md) |

See [CONTEXT.md](../CONTEXT.md) for the domain model and [`docs/adr/`](adr/) for every decision.
