// The selection file (versioned) says which variant of each scene is published;
// `publish` copies ONLY those from the review folder into docs/assets/readme/.
// Changing a line and publishing again swaps the published GIF without
// recording anything (ADR-0074 §5).
//
// Format — one scene per line, `#` starts a comment:
//
//     stats  a
//
// Every scene of the README has its line from the start, so adding a scene
// file never has to touch this one; picking another variant is a one-line edit.

import fs from "node:fs";
import path from "node:path";

export function parseSelection(text) {
  const selection = new Map();
  text.split("\n").forEach((raw, index) => {
    const line = raw.replace(/#.*/, "").trim();
    if (!line) return;
    const [scene, variant, ...extra] = line.split(/\s+/);
    if (!variant || extra.length > 0) throw new Error(`selection line ${index + 1}: expected "<scene> <variant>", got "${raw.trim()}"`);
    if (selection.has(scene)) throw new Error(`selection line ${index + 1}: scene "${scene}" selected twice`);
    selection.set(scene, variant);
  });
  return selection;
}

export function readSelection(file) {
  return parseSelection(fs.readFileSync(file, "utf8"));
}

/**
 * Copy each selected variant (GIF + poster) from `reviewDir/<scene>/` to
 * `assetsDir/<scene>.gif|.jpg`. A selected scene with no scene file yet is
 * skipped (it is planned, not built); a built scene whose selected variant was
 * never recorded is an error — publishing must never pick for the user.
 */
export function publish({ selection, scenes, reviewDir, assetsDir, only = null, log = console.log }) {
  const published = [];
  const skipped = [];
  fs.mkdirSync(assetsDir, { recursive: true });
  for (const [scene, variant] of selection) {
    if (only && !only.includes(scene)) continue;
    const definition = scenes.get(scene);
    if (!definition) {
      skipped.push({ scene, reason: "no scene file yet" });
      continue;
    }
    if (!definition.variants.some((v) => v.id === variant)) {
      throw new Error(`selection: scene "${scene}" has no variant "${variant}" (variants: ${definition.variants.map((v) => v.id).join(", ")})`);
    }
    const gif = path.join(reviewDir, scene, `${variant}.gif`);
    const poster = path.join(reviewDir, scene, `${variant}.jpg`);
    for (const file of [gif, poster]) {
      if (!fs.existsSync(file)) throw new Error(`selection: ${scene}/${variant} was never recorded (${file} missing) — run \`make readme-media SCENE=${scene}\``);
    }
    fs.copyFileSync(gif, path.join(assetsDir, `${scene}.gif`));
    fs.copyFileSync(poster, path.join(assetsDir, `${scene}.jpg`));
    published.push({ scene, variant });
    log(`published ${scene} ← variant ${variant}`);
  }
  for (const { scene, reason } of skipped) log(`skipped ${scene}: ${reason}`);
  return { published, skipped };
}
