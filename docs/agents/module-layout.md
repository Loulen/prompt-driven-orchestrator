# Module layout

Where a new file goes, and why the tree stays flat on purpose. This is the operating rule; the
decision that *refuses* the alternatives (sub-crates, sub-directories, name-prefix taxonomies) is
[ADR-0039](../adr/0039-le-daemon-reste-un-crate-unique-les-modules-restent-freres.md). The rule is
enforced by [`scripts/layout-ratchet.sh`](../../scripts/layout-ratchet.sh), run in CI.

> "Module layout" here is the physical shape of the source tree — files and modules. It is **not**
> the `layout`/`semantic` partitioning of pipeline nodes (that meaning of the word lives in
> CONTEXT.md). Say "module layout", never bare "layout".

## The rule

**One concern is one sibling file.** The daemon is a single crate whose modules are flat siblings
under `crates/pdo-daemon/src/`; the frontend's components are flat siblings under
`frontend/src/components/`. When you add behaviour, the default is to fold it into the existing
sibling that owns the concern — not to open a new top-level file.

A genuinely new concern *may* be a new sibling file. That is the exception, not the reflex, and it
is justified in the PR description. "This function is long" or "this feels separate" is not a new
concern; it is a function in the module that already owns its concern.

There are no directories inside `crates/pdo-daemon/src/`. `lib.rs` is carved by concern into
sibling modules, never by directory. The one directory under `frontend/src/components/` is `ui/`
(generated shadcn primitives) — see below.

## Watched directories and the ratchet

The ratchet counts **direct tracked files** in each watched directory (files git tracks at the top
level of that directory; files in sub-directories such as `ui/` are not counted). The count must
never exceed its baseline.

| Watched directory          | Baseline |
| -------------------------- | -------- |
| `frontend/src/components`  | 137      |
| `crates/pdo-daemon/src`    | 54       |

Colocated Rust unit tests (`#[cfg(test)] mod tests` at the bottom of a module) are **counted with
their module** — they are part of the same file, so a tidied module that absorbs a sibling drops the
count by the full file, tests included. There is no `*.test.*` exclusion: introducing one would be a
pattern that quietly drifts.

## Ratchet down

The ratchet is one-directional by intent. When you tidy a directory *below* its baseline (fold a
file into a sibling, delete dead code), the script prints a `note:` and you **lower the baseline in
the same commit**. The number only ever goes down without discussion; raising it requires the PR to
say why a new direct file is the right shape.

## What this rule is NOT

- **Not a ban on growth.** A new concern can be a new file. The ratchet makes that a deliberate,
  reviewed act instead of the path of least resistance.
- **Not a fixed cap.** There is no "max 30 entries" target — only "never more than today, and less
  when you tidy". The absolute number is not the point; the direction is.
- **Not a directory scheme.** Do not answer a large flat list by sorting files into folders. For the
  daemon that would move `module_path!()` and break `RUST_LOG` targets and runbook greps
  (ADR-0039); for the frontend the taxonomy question is a separate, human-gated piece of work
  (issues #338, #359). `ui/` is the sole directory under `components/`, and only because
  `npx shadcn add` re-flattens anything else back out.
- **Not barrel files.** No `index.ts` re-export hubs. Import from the file that owns the symbol.

## Discovery

There is no index of `docs/agents/*.md`. The channel that surfaces this rule at the moment it
matters is the ratchet's own failure message — it names this file when a directory grows past its
baseline.
