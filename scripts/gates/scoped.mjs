#!/usr/bin/env node
// `make scoped` (after `make fast`): only the checks the diff touches.
//
//   BASE=<ref>   diff against the merge-base with <ref> instead of the branch
//                this one started from
//   DRY_RUN=1    print the plan, run nothing
//
// The diff is the working tree (committed or not, untracked files included)
// against the merge-base. The plan is printed, step by step with its reason,
// then run; a failed step does not stop the others. The plan itself is the pure
// lib/plan.mjs, tested by `node --test`.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { pickBase, planScoped } from "./lib/plan.mjs";
import { runSteps, shellExec } from "./lib/run.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 64 << 20 }).trim();
const tryGit = (...args) => {
  try {
    return execFileSync("git", args, { cwd: root, encoding: "utf8", stdio: ["ignore", "pipe", "ignore"] }).trim();
  } catch {
    return null;
  }
};
const some = (files) => (files.length > 10 ? `${files.slice(0, 10).join(", ")} (+${files.length - 10} more)` : files.join(", "));
const lines = (text) => (text ? text.split("\n").filter(Boolean) : []);

/** `{ ref, why }`: BASE=, else the upstream when it is another branch, else the nearest local branch. */
function resolveBase() {
  if (process.env.BASE) return { ref: process.env.BASE, why: "BASE=" };
  const current = tryGit("symbolic-ref", "--quiet", "--short", "HEAD");
  const upstream = tryGit("rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}");
  if (upstream && current && upstream.replace(/^[^/]+\//, "") !== current) {
    return { ref: upstream, why: "the upstream of the branch" };
  }
  const candidates = lines(git("for-each-ref", "--format=%(refname:short) %(ahead-behind:HEAD)", "refs/heads"))
    .map((line) => line.split(" "))
    .filter(([ref]) => ref !== current)
    .map(([ref, ahead, behind]) => ({ ref, ahead: Number(ahead), behind: Number(behind) }));
  const ref = pickBase(candidates);
  if (!ref) throw new Error("no branch to diff against: set BASE=<ref>");
  return { ref, why: "the nearest local branch this one started from (BASE= to change it)" };
}

/** `{ name, dir, deps }` of every workspace member, from `cargo metadata` (compiles nothing). */
function workspaceCrates() {
  const meta = JSON.parse(
    execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps"], { cwd: root, encoding: "utf8", maxBuffer: 64 << 20 }),
  );
  const names = new Set(meta.packages.map((p) => p.name));
  return meta.packages.map((p) => ({
    name: p.name,
    dir: path.relative(root, path.dirname(p.manifest_path)),
    deps: [...new Set(p.dependencies.map((d) => d.name).filter((n) => names.has(n)))],
  }));
}

function rustFiles(dir) {
  return fs.readdirSync(dir, { withFileTypes: true, recursive: true })
    .filter((e) => e.isFile() && e.name.endsWith(".rs"))
    .map((e) => path.join(e.parentPath ?? e.path, e.name));
}

/** `{ path: [crate…] }`: the files outside a crate that the crate's sources `include_str!`/`include_bytes!`. */
function embeddedFiles(crates) {
  const embeds = {};
  const INCLUDE = /include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)/g;
  for (const crate of crates) {
    for (const file of rustFiles(path.join(root, crate.dir))) {
      for (const [, literal] of fs.readFileSync(file, "utf8").matchAll(INCLUDE)) {
        const target = path.relative(root, path.resolve(path.dirname(file), literal));
        if (target.startsWith(`${crate.dir}/`) || target.startsWith("..")) continue;
        embeds[target] = [...new Set([...(embeds[target] ?? []), crate.name])];
      }
    }
  }
  return embeds;
}

// A Rust type the front sees is a ts-rs one: `#[derive(… TS …)]` or a `#[ts(…)]` attribute.
const DECLARES_TS = /#\[ts\(|derive\([^)]*\bTS\b/;

/** The changed `.rs` files declaring a TS type, before (at `base`) or after the change. */
function typeFiles(changed, base) {
  return changed.filter((file) => {
    if (!file.endsWith(".rs")) return false;
    const now = path.join(root, file);
    if (fs.existsSync(now) && DECLARES_TS.test(fs.readFileSync(now, "utf8"))) return true;
    return DECLARES_TS.test(tryGit("show", `${base}:${file}`) ?? "");
  });
}

function scriptSuites() {
  return fs.readdirSync(path.join(root, "scripts"), { withFileTypes: true })
    .filter((e) => e.isDirectory() && fs.existsSync(path.join(root, "scripts", e.name, "test")))
    .map((e) => `scripts/${e.name}`);
}

async function main() {
  const base = resolveBase();
  const mergeBase = git("merge-base", "HEAD", base.ref);
  const changed = [
    ...new Set([
      ...lines(git("diff", "--name-only", "--no-renames", mergeBase)),
      ...lines(git("ls-files", "--others", "--exclude-standard")),
    ]),
  ];
  const crates = workspaceCrates();
  const plan = planScoped({
    changed,
    crates,
    embeds: embeddedFiles(crates),
    typeFiles: typeFiles(changed, mergeBase),
    scriptSuites: scriptSuites(),
    frontendInstalled: fs.existsSync(path.join(root, "frontend/node_modules")),
  });

  console.log(`\n==> Scoped gate: ${changed.length} changed file(s) since ${base.ref} (merge-base ${mergeBase.slice(0, 10)}), ${base.why}`);
  if (plan.docs.length) console.log(`    doc only, nothing beyond the fast gate: ${some(plan.docs)}`);
  if (plan.uncovered.length) console.log(`    no scoped step (make gate covers them): ${some(plan.uncovered)}`);
  if (!plan.steps.length) {
    console.log("    plan: nothing beyond the fast gate");
    return 0;
  }
  console.log("    plan:");
  for (const step of plan.steps) console.log(`      - ${step.name}: ${step.command}\n          why: ${step.reason}`);
  if (process.env.DRY_RUN === "1") return 0;
  return runSteps(plan.steps, { exec: shellExec(root) });
}

main().then(
  (code) => process.exit(code),
  (err) => {
    console.error(`scoped gate: ${err.message}`);
    process.exit(2);
  },
);
