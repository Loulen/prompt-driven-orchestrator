/**
 * The *Overview* tour's preparations and teardown (#911, spec #910, ADR-0071
 * §3–4), through verbs that know nothing about tours: the training repository,
 * the `tutorial-overview` pipeline, an example Trigger that is harmless by
 * construction, and a completed Run. The teardown removes the Trigger at any
 * exit; the pipeline and the Run stay the user's, as after *First run*.
 */

import { ApiError } from "../../shared/http/client";
import { createPipeline, fetchPipelines, savePipeline } from "../../pipeline/api/pipelines";
import { useEditStore } from "../../pipeline/store/editStore";
import { createRun } from "../../run/api/createRun";
import { fetchRun, fetchRuns } from "../../run/api/runs";
import { createTrigger, deleteTrigger, fetchTriggers, updateTrigger } from "../../trigger/api/triggers";
import { prepareRepo, TUTORIAL_REPO_PATH } from "./firstRunPreparation";
import {
  EXAMPLE_TRIGGER_CRON,
  EXAMPLE_TRIGGER_GUARD,
  EXAMPLE_TRIGGER_INPUT,
  EXAMPLE_TRIGGER_NAME,
  IMPLEMENTER_SCRIPT,
  OVERVIEW_PIPELINE_YAML,
  OVERVIEW_PROMPT,
  OVERVIEW_RUN_NAME,
  REVIEWER_SCRIPT,
  TUTORIAL_OVERVIEW_PIPELINE_ID,
} from "./overviewPipeline";


/**
 * What this pass of the tour prepared: the Run it visits and the Trigger it shows.
 * Set by the preparations, read by the steps — the Run is not always the newest
 * one in the list (a replay reuses an older one), so « the tour's Run » has to be
 * remembered rather than guessed.
 */
const prepared: { runId: string | null; triggerId: string | null } = { runId: null, triggerId: null };

/** The Run this pass of the tour is visiting, once its preparation has answered. */
export function overviewRunId(): string | null {
  return prepared.runId;
}

/** The example Trigger this pass of the tour installed or picked up. */
export function exampleTriggerId(): string | null {
  return prepared.triggerId;
}

/**
 * Share one in-flight call between the preparations that need it. They run in
 * parallel (the engine starts them together), but the Trigger and the Run can only
 * be made once the repository and the pipeline exist: both await the same
 * promise instead of racing a second `POST /repos/create`.
 */
function shared(fn: () => Promise<void>): () => Promise<void> {
  let flight: Promise<void> | null = null;
  return () => {
    flight ??= fn().finally(() => {
      flight = null;
    });
    return flight;
  };
}

const repoReady = shared(prepareRepo);

/**
 * Put `tutorial-overview` in the Library, or leave the one already there alone —
 * a user who edited it keeps their edit, the rule *First run* follows too.
 */
const pipelineReady = shared(async () => {
  const existing = await fetchPipelines();
  if (existing.some((p) => p.id === TUTORIAL_OVERVIEW_PIPELINE_ID)) return;
  try {
    await createPipeline(TUTORIAL_OVERVIEW_PIPELINE_ID);
  } catch (e) {
    // Lost the race with another tab: the pipeline exists, which is all we promised.
    if (!(e instanceof ApiError) || e.status !== 409) throw e;
    return;
  }
  await savePipeline(TUTORIAL_OVERVIEW_PIPELINE_ID, OVERVIEW_PIPELINE_YAML, {
    implementer: IMPLEMENTER_SCRIPT,
    reviewer: REVIEWER_SCRIPT,
  });
});

export async function prepareRepoStep(): Promise<void> {
  await repoReady();
}

/**
 * The Library list is loaded once, at mount, and the daemon announces no
 * creation — so a pipeline this preparation just saved would stay out of the
 * Pipelines tab until a reload, and step 15 would point at nothing (FP #911).
 * The store is asked to re-read it, the way the New/Import buttons do.
 */
export async function preparePipelineStep(): Promise<void> {
  await pipelineReady();
  await useEditStore.getState().loadPipelines();
}

/**
 * The example Trigger, active, picked up by its name when a previous pass left
 * it behind (a tab closed mid-tour skips the teardown). A picked-up Trigger is
 * put back in the shape the tour shows — active, guard `false` — because it is the
 * tour's object, not the user's.
 */
export async function prepareExampleTrigger(): Promise<void> {
  await Promise.all([repoReady(), pipelineReady()]);
  const existing = (await fetchTriggers()).find((t) => t.name === EXAMPLE_TRIGGER_NAME);
  if (existing) {
    if (!existing.enabled || (existing.guard_command ?? "").trim() !== EXAMPLE_TRIGGER_GUARD) {
      await updateTrigger(existing.id, { enabled: true, guard_command: EXAMPLE_TRIGGER_GUARD });
    }
    prepared.triggerId = existing.id;
    return;
  }
  const created = await createTrigger({
    name: EXAMPLE_TRIGGER_NAME,
    pipeline_id: TUTORIAL_OVERVIEW_PIPELINE_ID,
    cron: EXAMPLE_TRIGGER_CRON,
    input_template: EXAMPLE_TRIGGER_INPUT,
    target_repo: TUTORIAL_REPO_PATH,
    guard_command: EXAMPLE_TRIGGER_GUARD,
    overlap_policy: "skip",
    auto_name: false,
  });
  prepared.triggerId = created.id;
}

/** How often the Run is re-read while the preparation waits for it. */
export const RUN_POLL_MS = 1_000;
/**
 * How long the preparation waits for the Run. Two scripts under the 60 s wrapper
 * timeout each: a Run still going after three minutes is stuck, and a card that
 * waited forever would be the infinite wait story 7 refuses.
 */
export const RUN_WAIT_MS = 180_000;

function abortError(): Error {
  return new DOMException("The tour was closed.", "AbortError");
}

function sleep(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) return reject(abortError());
    const timer = setTimeout(() => {
      signal?.removeEventListener("abort", onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      reject(abortError());
    };
    signal?.addEventListener("abort", onAbort, { once: true });
  });
}

/**
 * Read the Run until it is `completed`. Any other terminal or waiting state is a
 * refusal quoting the daemon's reason (spec #910): a tour that visited a failed
 * Run would be explaining outputs that were never written.
 */
async function waitForCompleted(runId: string, signal?: AbortSignal): Promise<void> {
  const deadline = Date.now() + RUN_WAIT_MS;
  for (;;) {
    if (signal?.aborted) throw abortError();
    const run = await fetchRun(runId);
    if (run.status === "completed") return;
    if (run.status !== "running") {
      const reason = run.failure_reason ?? run.awaiting_reason ?? null;
      throw new Error(
        `Run ${run.name?.trim() || runId} ended \`${run.status}\` instead of \`completed\`${reason ? `: ${reason}` : "."}`,
      );
    }
    if (Date.now() >= deadline) {
      throw new Error(
        `Run ${run.name?.trim() || runId} was still running after ${RUN_WAIT_MS / 60_000} minutes.`,
      );
    }
    await sleep(RUN_POLL_MS, signal);
  }
}

/**
 * The Run the tour visits (spec #910, design Q8): the newest `tutorial-overview`
 * Run that is `completed` and not archived — so replaying the tour does not stack
 * identical Runs — else a fresh one, waited for until `completed`.
 *
 * One still `running` (a previous pass closed before it finished) is waited for
 * rather than doubled.
 */
export async function prepareCompletedRun(signal?: AbortSignal): Promise<void> {
  prepared.runId = null;
  const runs = await fetchRuns();
  const ours = runs.filter((r) => r.pipeline_name === TUTORIAL_OVERVIEW_PIPELINE_ID && !r.triggered_by);
  const reusable = ours.find((r) => r.status === "completed") ?? ours.find((r) => r.status === "running");
  let runId = reusable?.run_id ?? null;
  if (!runId) {
    await Promise.all([repoReady(), pipelineReady()]);
    if (signal?.aborted) throw abortError();
    const created = await createRun({
      pipeline: TUTORIAL_OVERVIEW_PIPELINE_ID,
      pipeline_id: TUTORIAL_OVERVIEW_PIPELINE_ID,
      input: OVERVIEW_PROMPT,
      variables: {},
      target_repo: TUTORIAL_REPO_PATH,
      name: OVERVIEW_RUN_NAME,
      auto_name: false,
      // No sandbox: two bash scripts that echo and write their outputs need no
      // container, and an instance without Docker must still play the tour.
      sandbox: "off",
    });
    runId = created.run_id;
  }
  await waitForCompleted(runId, signal);
  prepared.runId = runId;
}

// ---- teardown ---------------------------------------------------------------

/**
 * Remove the example Trigger — every one of that name, so a Trigger a previous
 * pass left behind goes too. A 404 is a success (`deleteTrigger` says so).
 */
export async function removeExampleTrigger(): Promise<void> {
  const triggers = await fetchTriggers();
  await Promise.all(
    triggers.filter((t) => t.name === EXAMPLE_TRIGGER_NAME).map((t) => deleteTrigger(t.id)),
  );
  prepared.triggerId = null;
}
