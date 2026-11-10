// The mocked history on disk: what a real run leaves for the run view to open.

import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { writeRunPipelines } from "../lib/history-write.mjs";
import { planHistory } from "../lib/history-plan.mjs";
import { readTarget, withName } from "../lib/targets.mjs";

test("each mocked run gets its pipeline and prompts where the daemon reads a run's pipeline", (t) => {
  const repo = fs.mkdtempSync(path.join(os.tmpdir(), "history-write-"));
  t.after(() => fs.rmSync(repo, { recursive: true, force: true }));
  const plan = planHistory({ now: new Date("2026-09-22T20:00:00Z"), targetRepo: repo, triggerId: "trg-demo" });
  writeRunPipelines(plan);

  const started = plan.events.filter((e) => e.kind === "run_started");
  assert.ok(started.length > 100);
  for (const { run_id: runId, payload } of started) {
    const dir = path.join(repo, ".pdo", "runs", runId);
    const { yaml, prompts } = readTarget(payload.pipeline_id);
    // GET /runs/<id>/pipeline reads <target_repo>/.pdo/runs/<id>/pipeline.yaml and its prompts.
    assert.equal(fs.readFileSync(path.join(dir, "pipeline.yaml"), "utf8"), withName(yaml, payload.pipeline_name));
    for (const [node, content] of Object.entries(prompts)) {
      assert.equal(fs.readFileSync(path.join(dir, "pipeline.prompts", `${node}.md`), "utf8"), content);
    }
  }
});
