#!/usr/bin/env node
// `make release-bump` (ADR-0095):
// at the merge of an `integration/*` branch into `main`, propose the semver
// level from the commits since the last tag, then bump every version, release
// the CHANGELOG's `## Unreleased` and number the draft ADRs.
//
//   LEVEL=patch|minor|major    force the level of the workspace
//   API_LEVEL=patch|minor|major force the level of the extension API
//   DRY_RUN=1                  print the plan, write nothing
//
// The pure parts live in lib/ and are tested by `node --test`.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { numberDrafts, rewriteCitations } from "./lib/adr.mjs";
import { releaseChangelog } from "./lib/changelog.mjs";
import { LEVELS, apiLevel, bump, proposeLevel } from "./lib/level.mjs";
import { VERSION_FILES, readVersions, writeVersions } from "./lib/versions.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const ADR_DIR = "docs/adr";
const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" });

function forced(name) {
  const value = process.env[name];
  if (!value) return null;
  if (!LEVELS.includes(value)) throw new Error(`${name}=${value}: patch, minor or major`);
  return value;
}

/** The commits since the last `v*` tag, each with the files it touches. */
function commitsSince(tag) {
  const SEP = "\u0001";
  const log = git("log", `${tag}..HEAD`, "--name-only", `--format=${SEP}%B${SEP}`);
  const chunks = log.split(SEP).slice(1);
  const commits = [];
  for (let i = 0; i + 1 < chunks.length; i += 2) {
    commits.push({ message: chunks[i].trim(), files: chunks[i + 1].split("\n").filter(Boolean) });
  }
  return commits;
}

function main() {
  const tag = git("describe", "--tags", "--abbrev=0", "--match", "v*").trim();
  const commits = commitsSince(tag);
  const proposed = proposeLevel(commits);
  const level = forced("LEVEL") ?? proposed.level;
  if (!level) throw new Error(`no commit since ${tag}: nothing to release (LEVEL= forces one)`);

  const files = Object.fromEntries(VERSION_FILES.map((f) => [f, fs.readFileSync(path.join(root, f), "utf8")]));
  const current = readVersions(files);
  const api = forced("API_LEVEL")
    ? { level: forced("API_LEVEL"), why: "API_LEVEL" }
    : { ...apiLevel(commits, current.api), why: null };
  api.why ??= api.reasons.length ? `from: ${api.reasons.join("; ")}` : "no commit touches the API";
  const next = { workspace: bump(current.workspace, level), api: bump(current.api, api.level) };

  const changelog = releaseChangelog(fs.readFileSync(path.join(root, "CHANGELOG.md"), "utf8"), next.workspace);
  const renames = numberDrafts(fs.readdirSync(path.join(root, ADR_DIR)));

  console.log(`Since ${tag}: ${commits.length} commit${commits.length === 1 ? "" : "s"}.`);
  console.log(`Proposed level: ${proposed.level ?? "none"}${proposed.reasons.length ? ", from:" : ""}`);
  for (const reason of proposed.reasons) console.log(`  - ${reason}`);
  if (forced("LEVEL")) console.log(`Forced level: ${level} (LEVEL=${level})`);
  console.log(`Workspace: ${current.workspace} -> ${next.workspace} (${level})`);
  console.log(`Extension API and shared module: ${current.api} -> ${next.api} (${api.level}, ${api.why})`);
  console.log(`CHANGELOG: ## Unreleased -> ## ${next.workspace}`);
  for (const r of renames) console.log(`ADR: ${ADR_DIR}/${r.from} -> ${ADR_DIR}/${r.to} (${r.id})`);
  if (process.env.DRY_RUN) {
    console.log("DRY_RUN: nothing written.");
    return;
  }

  for (const [file, text] of Object.entries(writeVersions(files, next))) fs.writeFileSync(path.join(root, file), text);
  fs.writeFileSync(path.join(root, "CHANGELOG.md"), changelog);
  // Cargo.lock: cargo rewrites the versions of the workspace's own packages.
  execFileSync("cargo", ["update", "--workspace", "--offline", "--quiet"], { cwd: root, stdio: "inherit" });

  for (const r of renames) fs.renameSync(path.join(root, ADR_DIR, r.from), path.join(root, ADR_DIR, r.to));
  if (renames.length) {
    const rewritten = [];
    for (const file of git("ls-files", "--cached", "--others", "--exclude-standard").split("\n").filter(Boolean)) {
      const full = path.join(root, file);
      if (!fs.existsSync(full) || !fs.statSync(full).isFile()) continue;
      const text = fs.readFileSync(full, "utf8");
      if (text.includes("\u0000")) continue;
      const out = rewriteCitations(text, renames);
      if (out !== text) {
        fs.writeFileSync(full, out);
        rewritten.push(file);
      }
    }
    console.log(`Citations rewritten in ${rewritten.length} files${rewritten.length ? `: ${rewritten.join(", ")}` : ""}.`);
  }
  console.log(`\nReleased ${next.workspace}. Check \`git diff\`, then commit: chore(release): v${next.workspace}`);
  const undo = ["git checkout -- ."];
  if (renames.length) undo.push(`rm ${renames.map((r) => `${ADR_DIR}/${r.to}`).join(" ")}`);
  console.log(`Another level: \`${undo.join(" && ")}\`, then \`make release-bump LEVEL=…\`.`);
}

try {
  main();
} catch (error) {
  console.error(`release-bump: ${error.message}`);
  process.exit(1);
}
