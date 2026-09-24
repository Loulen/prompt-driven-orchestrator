// Seed the demo instance with its mocked history (ADR-0074 §3): the demo
// trigger (through the API, so it is a real row the Triggers tab shows), then
// the planned events, transcripts, fires and manual prices. The trigger fires
// `prod-check`, a pipeline of its own: never `implement-review`, the pipeline
// every other scene shows.

import { PROD_CHECK, planHistory } from "./history-plan.mjs";
import { writeHistory } from "./history-write.mjs";

export const DEMO_TRIGGER = {
  name: "prod-health-check",
  cron: "* * * * *",
  guard_command: "./prod-health-check.sh",
};

export async function seedHistory(instance, { now = new Date() } = {}) {
  const trigger = await instance.api("POST", "/triggers", {
    name: DEMO_TRIGGER.name,
    pipeline_id: PROD_CHECK.id,
    target_repo: instance.repo,
    input_template: "{{guard_stdout}}",
    cron: DEMO_TRIGGER.cron,
    guard_command: DEMO_TRIGGER.guard_command,
  });
  // Disabled: the history is mocked, and an armed every-minute trigger would
  // launch real runs mid-recording. A scene that films it enabled flips it.
  await instance.api("PATCH", `/triggers/${trigger.id}`, { enabled: false });

  const plan = planHistory({ now, targetRepo: instance.repo, triggerId: trigger.id });
  writeHistory(plan, { home: instance.home, dbPath: instance.dbPath, triggerId: trigger.id });
  return {
    triggerId: trigger.id,
    runs: new Set(plan.events.map((e) => e.run_id)).size,
    fires: plan.fires.length,
    incidents: plan.prodChecks.filter((p) => p.incidentFound).length,
  };
}
