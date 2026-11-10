// The core's media config (lib/config.mjs): the first consumer of the tool.
// `make readme-media` without CONFIG reads this file. Another repo writes its
// own and calls `make readme-media CONFIG=<its file>` from a checkout of the
// core (scripts/readme-media/README.md, « From another repo »).

import { installTargets } from "./lib/targets.mjs";
import { seedHistory } from "./lib/seed.mjs";

export default {
  binary: "../../target/debug/pdo",
  scenes: "./scenes",
  fixture: {
    // ~/code/shop-app in the demo HOME.
    repo: "./fixture/shop-app",
    // The target pipelines, installed as drawn (lib/targets.mjs, ADR-0074 §6).
    pipelines: (libraryDir) => installTargets(libraryDir),
  },
  selection: "./selection.txt",
  review: "../../.readme-media",
  destination: "../../docs/assets/readme",
  // The core's bootstrap: the mocked history (ADR-0074 §3), for the scenes that
  // declare `needs: ["history"]`.
  async seed(instance, scene) {
    if (!(scene.needs ?? []).includes("history")) return;
    const seeded = await seedHistory(instance);
    console.log(`mocked history: ${seeded.runs} runs, ${seeded.fires} trigger fires (${seeded.incidents} real incidents)`);
  },
  playwright: "../../frontend",
};
