// The measures of `make bench-gates` for one ref, from the targets its Makefile
// declares: a ref older than a gate gets « n/a » for it, never a failure.

/** A production file of 401 lines, one over the limit of the file-size check. */
export const OVERSIZED_FILE = "crates/pdo/src/bench_oversized.rs";
export const OVERSIZED_LINES = 401;

/** The content of the injected file: 401 production lines, not referenced by any `mod`. */
export function oversizedSource(lines = OVERSIZED_LINES) {
  return Array.from({ length: lines }, (_, i) => `pub const LINE_${i + 1}: u32 = ${i + 1};`).join("\n") + "\n";
}

/** The front file the « front-only diff » of the scoped gate appends a comment to. */
export const FRONT_FILE = "frontend/src/main.tsx";

/** The targets a Makefile declares (`name:` at the start of a line; `:=` is a variable). */
export function makeTargets(makefile) {
  const targets = new Set();
  for (const line of makefile.split("\n")) {
    const m = /^([A-Za-z0-9_.-]+(?:\s+[A-Za-z0-9_.-]+)*)\s*:(?![:=])/.exec(line);
    if (!m) continue;
    for (const name of m[1].split(/\s+/)) if (!name.startsWith(".")) targets.add(name);
  }
  return targets;
}

/**
 * The measures, in the order they run, from a fresh worktree:
 * `{ id, label, command, setup?, cleanup?, expect, wrapped?, na? }`.
 * - `setup` / `cleanup`: shell lines run before / after, not timed;
 * - `expect`: "pass", or "fail" for the injected structural failure;
 * - `wrapped`: the build runs under the bench's private sccache;
 * - `na`: why the measure does not exist at this ref (nothing runs);
 * - `showCommand`: the command depends on the ref, so its cell names it.
 * The structural failure runs first, on the cold target/ an agent starts from.
 */
export function benchMeasures({ targets, sccache }) {
  const has = (t) => targets.has(t);
  const structural = has("fast") ? "make fast" : "make test";
  return [
    {
      id: "frontend-deps",
      label: "Frontend deps (pnpm install)",
      command: "cd frontend && pnpm install --frozen-lockfile",
      expect: "pass",
    },
    {
      id: "structural",
      label: `First structural failure (${OVERSIZED_LINES}-line file)`,
      showCommand: true,
      command: structural,
      setup: `cat > ${OVERSIZED_FILE} <<'BENCH_EOF'\n${oversizedSource()}BENCH_EOF`,
      cleanup: `rm -f ${OVERSIZED_FILE}`,
      expect: "fail",
      marker: "bench_oversized",
    },
    {
      id: "cold-build",
      label: sccache ? "Cold build (`cargo build`, empty sccache)" : "Cold build (`cargo build`)",
      command: "cargo build",
      setup: "cargo clean",
      expect: "pass",
      wrapped: sccache,
    },
    {
      id: "warm-build",
      label: "Build with a warm sccache (`cargo build` after `cargo clean`)",
      command: "cargo build",
      setup: "cargo clean",
      expect: "pass",
      wrapped: true,
      na: sccache ? undefined : "n/a (sccache absent)",
    },
    {
      id: "scoped-front",
      label: "Scoped gate, front-only diff (`make scoped BASE=HEAD`)",
      command: "make scoped BASE=HEAD",
      setup: `printf '\\n// bench-gates: a front-only diff\\n' >> ${FRONT_FILE}`,
      cleanup: `git checkout -- ${FRONT_FILE}`,
      expect: "pass",
      na: has("scoped") ? undefined : "n/a (no `make scoped` at this ref)",
    },
    {
      id: "full-suite",
      label: "Full suite",
      showCommand: true,
      command: has("gate") ? "make gate" : "make test",
      expect: "pass",
    },
  ];
}
