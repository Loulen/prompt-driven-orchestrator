// The manifest of the review folder: for every scene and variant, the facts a
// maintainer checks a regeneration on without opening each GIF — duration,
// weight, dimensions, markers. Recording one scene (SCENE=…) or one variant
// (VARIANT=…) replaces those entries and keeps the others'.
//
// A variant is encoded in its work folder, then `landVariant` moves its GIF and
// poster into the review folder and writes its entry in one synchronous step:
// a run cut short (Ctrl+C, crash) leaves the previous files and entry together,
// never a new GIF under a stale entry.

import fs from "node:fs";
import path from "node:path";

export function readManifest(file) {
  try {
    return JSON.parse(fs.readFileSync(file, "utf8"));
  } catch {
    return { scenes: {} };
  }
}

export function updateManifest(file, sceneName, entry) {
  const manifest = readManifest(file);
  manifest.generated_at = new Date().toISOString();
  const previous = manifest.scenes?.[sceneName]?.variants ?? [];
  const recorded = new Set(entry.variants.map((v) => v.id));
  const variants = [...previous.filter((v) => !recorded.has(v.id)), ...entry.variants].sort((a, b) => a.id.localeCompare(b.id));
  manifest.scenes = { ...manifest.scenes, [sceneName]: { ...entry, variants } };
  manifest.scenes = Object.fromEntries(Object.entries(manifest.scenes).sort(([a], [b]) => a.localeCompare(b)));
  fs.writeFileSync(file, `${JSON.stringify(manifest, null, 2)}\n`);
  return manifest;
}

/** Move a finished variant's files (`[[from, to], …]`) into the review folder
 *  and record its entry. Synchronous on purpose: no signal handler can run
 *  between the move and the manifest write. */
export function landVariant({ file, sceneName, title, entry, moves }) {
  for (const [from, to] of moves) {
    fs.mkdirSync(path.dirname(to), { recursive: true });
    fs.renameSync(from, to);
  }
  return updateManifest(file, sceneName, { title, variants: [entry] });
}

/** One variant's manifest entry. Warnings say what misses the target, then
 *  what the take itself reported (`sceneWarnings`, `ctx.warn`). */
export function variantEntry({ variant, facts, plan, gif, poster, posterBytes, target, sceneWarnings = [] }) {
  const warnings = [];
  if (facts.durationS < target.min / 1000 || facts.durationS > target.max / 1000) {
    warnings.push(`duration ${facts.durationS}s outside ${target.min / 1000}–${target.max / 1000}s`);
  }
  const names = plan.markers.map((m) => m.name);
  const missing = variant.markers.filter((name) => !names.includes(name));
  if (missing.length > 0) warnings.push(`missing markers: ${missing.join(", ")}`);
  warnings.push(...sceneWarnings);
  return {
    id: variant.id,
    label: variant.label ?? variant.id,
    gif,
    poster,
    duration_s: facts.durationS,
    bytes: facts.bytes,
    poster_bytes: posterBytes,
    width: facts.width,
    height: facts.height,
    markers: plan.markers.map((m) => ({ name: m.name, t_s: Number((m.t / 1000).toFixed(2)) })),
    expected_markers: variant.markers,
    warnings,
  };
}
