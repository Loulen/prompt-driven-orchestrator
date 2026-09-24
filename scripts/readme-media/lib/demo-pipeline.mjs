// The demo pipeline `implement-review`, complete (with its loop), as every live
// scene runs it and the mocked history snapshots it. Nothing is written by hand
// here: the run snapshot (`run_started`'s `node_defs` / `edges`) is derived
// from the maintainer's target, fixture/targets/implement-review.yaml
// (lib/targets.mjs). Redraw the target, and the history follows.

import { DEMO_PIPELINE_ID, runSnapshot, targetPipeline } from "./targets.mjs";

export const DEMO_PIPELINE = runSnapshot(targetPipeline(DEMO_PIPELINE_ID), { id: DEMO_PIPELINE_ID });
