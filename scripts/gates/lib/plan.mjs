// The scoped gate's plan (ADR draft-les-controles-structurels-tournent-dans-un-crate-tidy-avant-toute-suite-longue):
// the changed files -> the steps that check them, each with the reason it was kept.
// Pure: what it needs from the repository (crate graph, embedded files, files
// declaring a TS type, script suites) is handed in by scoped.mjs.

const GENERATED_TS = "frontend/src/shared/generated/";
const WORKSPACE_FILES = new Set(["Cargo.toml", "Cargo.lock", "rust-toolchain", "rust-toolchain.toml"]);
const SHOWN = 3;

const listed = (files) =>
  files.length > SHOWN ? `${files.slice(0, SHOWN).join(", ")} (+${files.length - SHOWN} more)` : files.join(", ");

const isDoc = (file) => file.endsWith(".md") || file.startsWith("docs/");

/** Every crate that depends on one of `roots`, directly or not, dev-dependencies included, plus the roots. */
function rdeps(roots, crates) {
  const dependents = new Map(crates.map((c) => [c.name, []]));
  for (const crate of crates) for (const dep of crate.deps) dependents.get(dep)?.push(crate.name);
  const seen = new Set(roots);
  const queue = [...roots];
  while (queue.length) {
    for (const next of dependents.get(queue.shift()) ?? []) {
      if (!seen.has(next)) {
        seen.add(next);
        queue.push(next);
      }
    }
  }
  return [...seen].sort();
}

function crateOf(file, crates) {
  return crates.find((c) => file.startsWith(`${c.dir}/`))?.name;
}

/**
 * The plan of the scoped gate for `changed` (repo-relative paths).
 *
 * - `crates`: the workspace members, `{ name, dir, deps }` (deps: workspace crates only).
 * - `embeds`: `{ path: [crate…] }`, the files outside a crate that a crate embeds (`include_str!`).
 * - `typeFiles`: the changed Rust files that declare a type exported to the front (ts-rs).
 * - `scriptSuites`: the `scripts/<name>` folders that have a `test/` suite.
 * - `frontendInstalled`: whether `frontend/node_modules` exists.
 *
 * Returns `{ steps: [{ name, command, reason }], docs, uncovered }`: `docs` adds nothing
 * beyond the fast gate, `uncovered` is what no rule knows (the full gate covers it).
 */
export function planScoped({ changed, crates, embeds = {}, typeFiles = [], scriptSuites = [], frontendInstalled = true }) {
  const front = [];
  const generated = [];
  const types = [];
  const rustRoots = new Map(); // crate -> why
  const workspace = [];
  const scripts = new Map(); // suite -> files
  const docs = [];
  const uncovered = [];
  const typeSet = new Set(typeFiles);

  const touchCrate = (name, why) => rustRoots.set(name, [...(rustRoots.get(name) ?? []), why]);

  for (const file of changed) {
    if (typeSet.has(file)) types.push(file);
    const crate = crateOf(file, crates);
    if (WORKSPACE_FILES.has(file) || file.startsWith(".cargo/")) workspace.push(file);
    else if (crate) touchCrate(crate, file);
    else if (embeds[file]?.length) for (const c of embeds[file]) touchCrate(c, `${file}, embedded by ${c}`);
    else if (file.startsWith("frontend/")) {
      front.push(file);
      if (file.startsWith(GENERATED_TS)) generated.push(file);
    } else if (file.startsWith("scripts/")) {
      const suite = scriptSuites.find((s) => file.startsWith(`${s}/`));
      if (suite) scripts.set(suite, [...(scripts.get(suite) ?? []), file]);
      else uncovered.push(file);
    } else if (isDoc(file)) docs.push(file);
    else uncovered.push(file);
  }

  const steps = [];
  if (front.length) {
    const why = `frontend changed: ${listed(front)}`;
    if (!frontendInstalled) {
      steps.push({ name: "frontend deps", command: "cd frontend && pnpm install --frozen-lockfile", reason: "frontend/node_modules is missing" });
    }
    steps.push({ name: "typecheck", command: "cd frontend && pnpm run typecheck", reason: why });
    steps.push({ name: "lint", command: "cd frontend && pnpm run lint", reason: why });
    steps.push({ name: "vitest", command: "cd frontend && pnpm test", reason: why });
  }
  if (types.length || generated.length) {
    const why = [
      types.length && `a type exported to the front changed: ${listed(types)}`,
      generated.length && `generated bindings edited: ${listed(generated)}`,
    ].filter(Boolean);
    steps.push({ name: "types-check", command: "make --no-print-directory types-check", reason: why.join("; ") });
  }
  if (workspace.length) {
    const why = `workspace manifest changed: ${listed(workspace)}`;
    steps.push({ name: "nextest", command: "cargo nextest run --workspace --no-fail-fast", reason: why });
    steps.push({ name: "clippy", command: "cargo clippy --workspace --all-targets -- -D warnings", reason: why });
  } else if (rustRoots.size) {
    const roots = [...rustRoots.keys()].sort();
    const packages = rdeps(roots, crates);
    const flags = packages.map((p) => `-p ${p}`).join(" ");
    const why = `${roots.map((r) => `rdeps(${r}) from ${listed(rustRoots.get(r))}`).join("; ")} -> ${packages.join(", ")}`;
    steps.push({ name: "nextest", command: `cargo nextest run ${flags} --no-fail-fast`, reason: why });
    steps.push({ name: "clippy", command: `cargo clippy ${flags} --all-targets -- -D warnings`, reason: why });
  }
  for (const [suite, files] of scripts) {
    steps.push({
      name: `${suite} tests`,
      command: `node --disable-warning=ExperimentalWarning --test '${suite}/test/*.test.mjs'`,
      reason: `scripts changed: ${listed(files)}`,
    });
  }
  return { steps, docs, uncovered };
}

/**
 * The default base of the scoped gate: among the local branches, `{ ref, ahead, behind }`
 * relative to HEAD (`git for-each-ref --format='%(ahead-behind:HEAD)'`), the one HEAD
 * started from — the nearest (fewest commits of HEAD it lacks). A branch that already
 * contains every commit of HEAD and more (a branch started from this one, or the integration
 * this one was merged into) would hide the work: never a base.
 */
export function pickBase(candidates) {
  const eligible = candidates.filter((c) => !(c.behind === 0 && c.ahead > 0));
  eligible.sort((a, b) => a.behind - b.behind || a.ahead - b.ahead || a.ref.localeCompare(b.ref));
  return eligible[0]?.ref ?? null;
}
