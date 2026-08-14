import { describe, it, expect } from "vitest";
import {
  buildRunPayload,
  buildTriggerCreatePayload,
  buildTriggerUpdatePayload,
  buildVariables,
  canCreateTrigger,
  canLaunch,
  hasRequiredPrompt,
  overrideCount,
  parseVariableValue,
  promptOptional,
  resolvedCron,
  sandboxState,
  triggerInputRejectReason,
} from "./newRunForm";
import type { InstanceSettings, PipelineListEntry } from "../types";

function pipeline(over: Partial<PipelineListEntry> = {}): PipelineListEntry {
  return {
    id: "p1",
    name: "Auditor",
    scope: "repo",
    path: "/repo/.pdo/pipelines/auditor.yaml",
    node_count: 3,
    modified: null,
    variables: {},
    ...over,
  };
}

function settings(over: Partial<InstanceSettings> = {}): InstanceSettings {
  return {
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness: { effective: null, source: "default", stored: null, env: null, default: null },
    default_harness_model: { effective: {}, stored: {} },
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    home: "/home/user",
    autocomplete_turn_end: { effective: false, source: "default", stored: null, env: null, default: false },
    default_auto_name: { effective: true, source: "default", stored: null, env: null, default: true },
    price_table: { manual_path: null, fetched_path: null, source: null, fetched_at: null, fetched_rows: 0, manual_keys: [], reason: null },
    updated_at: "2026-07-01T10:00:00.000Z",
    ...over,
  };
}

/** The default_sandbox tier resolving to `name` (the #452 fixture). */
const defaultIs = (name: string, reason: string | null = null): Partial<InstanceSettings> => ({
  default_sandbox: { effective: name, source: "stored", stored: name, env: null, default: "off", reason },
});

describe("parseVariableValue", () => {
  it("parses int, falling back to 0 on garbage", () => {
    expect(parseVariableValue("42", "int")).toBe(42);
    expect(parseVariableValue("nope", "int")).toBe(0);
  });

  it("parses float, falling back to 0 on garbage", () => {
    expect(parseVariableValue("2.5", "float")).toBe(2.5);
    expect(parseVariableValue("", "float")).toBe(0);
  });

  // Only the exact string `true` is true — a checkbox never reaches here, so a typo
  // must read as false rather than as "any non-empty string".
  it("parses bool from the literal string true only", () => {
    expect(parseVariableValue("true", "bool")).toBe(true);
    expect(parseVariableValue("True", "bool")).toBe(false);
    expect(parseVariableValue("1", "bool")).toBe(false);
  });

  it("parses a list from JSON", () => {
    expect(parseVariableValue('["a", "b"]', "list")).toEqual(["a", "b"]);
  });

  it("falls back to a trimmed comma split for a non-JSON list", () => {
    expect(parseVariableValue("[a, b , c]", "list")).toEqual(["a", "b", "c"]);
    expect(parseVariableValue("a,b", "list")).toEqual(["a", "b"]);
  });

  it("passes an unknown type through as the raw string", () => {
    expect(parseVariableValue("  spaced  ", "string")).toBe("  spaced  ");
  });
});

/**
 * Run mode and Trigger mode each carried a byte-identical copy of this loop before #359.
 * The rules it encodes are the ones a single copy must keep whole.
 */
describe("buildVariables", () => {
  const declared = pipeline({
    variables: {
      max_iter: { var_type: "int", default: 3 },
      label: { var_type: "string", default: "ready" },
    },
  });

  it("keeps only overrides the pipeline declares", () => {
    expect(buildVariables({ max_iter: "5", ghost: "x" }, declared)).toEqual({ max_iter: 5 });
  });

  it("drops an override equal to the declared default (not an override)", () => {
    expect(buildVariables({ max_iter: "3" }, declared)).toEqual({});
  });

  it("parses each value by its declared type", () => {
    expect(buildVariables({ max_iter: "5", label: "rfa" }, declared)).toEqual({
      max_iter: 5,
      label: "rfa",
    });
  });

  it("returns an empty map when nothing was typed", () => {
    expect(buildVariables({}, declared)).toEqual({});
  });

  /**
   * The dedup, asserted where it can actually be observed: the payload builders CARRY the
   * map they are handed — they do not recompute it — so the single `buildVariables` call is
   * the only place the rules above live. `toBe` (identity), not `toEqual`: a builder that
   * rebuilt an equal map would pass an equality check and re-open the drift.
   */
  it("is computed once and carried into every payload, run and trigger alike", () => {
    const variables = buildVariables({ max_iter: "5" }, declared);
    const runFields = {
      selectedPipeline: declared,
      input: "audit",
      variables,
      targetRepo: "/repo",
      sourceBranch: "main",
      autoName: true,
      runName: "",
      sandbox: "",
      images: [],
    };
    const triggerFields = {
      selectedPipeline: declared,
      triggerName: "Nightly",
      resolvedCron: "0 * * * *",
      input: "audit",
      guardCommand: "",
      targetRepo: "/repo",
      sourceBranch: "main",
      allowOverlap: false,
      maxConcurrent: "",
      sandbox: "",
      autoName: true,
      variables,
    };
    expect(buildRunPayload(runFields).variables).toBe(variables);
    expect(buildTriggerCreatePayload(triggerFields).variables).toBe(variables);
    expect(buildTriggerUpdatePayload(triggerFields).variables).toBe(variables);
  });
});

describe("overrideCount", () => {
  const declared = pipeline({
    variables: { max_iter: { var_type: "int", default: 3 } },
  });

  it("counts nothing without a selected pipeline", () => {
    expect(overrideCount({ max_iter: "5" }, undefined)).toBe(0);
  });

  it("ignores undeclared keys and values equal to the default", () => {
    expect(overrideCount({ max_iter: "3", ghost: "x" }, declared)).toBe(0);
  });

  it("counts a real override", () => {
    expect(overrideCount({ max_iter: "5" }, declared)).toBe(1);
  });
});

describe("resolvedCron", () => {
  const daily = { rawCron: "", dailyHour: 9, dailyMinute: 0 };

  it("compiles the preset schedules", () => {
    expect(resolvedCron({ ...daily, cronPresetId: "every_15_min" })).toBe("*/15 * * * *");
    expect(resolvedCron({ ...daily, cronPresetId: "hourly" })).toBe("0 * * * *");
  });

  it("compiles the daily preset with its time of day", () => {
    expect(
      resolvedCron({ cronPresetId: "daily", rawCron: "", dailyHour: 7, dailyMinute: 30 }),
    ).toBe("30 7 * * *");
  });

  it("takes the raw expression, trimmed, for the custom escape hatch", () => {
    expect(
      resolvedCron({ cronPresetId: "custom", rawCron: "  */5 * * * *  ", dailyHour: 9, dailyMinute: 0 }),
    ).toBe("*/5 * * * *");
  });

  // Drives the "cron: —" placeholder and blocks Create: a blank custom expression is not
  // a schedule.
  it("resolves an empty custom expression to the empty string", () => {
    expect(resolvedCron({ ...daily, cronPresetId: "custom" })).toBe("");
  });
});

describe("promptOptional / hasRequiredPrompt", () => {
  it("treats an absent prompt_required as required (#158 default)", () => {
    expect(promptOptional(pipeline())).toBe(false);
    expect(promptOptional(undefined)).toBe(false);
  });

  it("is optional only for an explicit prompt_required: false", () => {
    expect(promptOptional(pipeline({ prompt_required: false }))).toBe(true);
    expect(promptOptional(pipeline({ prompt_required: true }))).toBe(false);
  });

  it("demands non-blank input when the prompt is required", () => {
    expect(hasRequiredPrompt(false, "")).toBe(false);
    expect(hasRequiredPrompt(false, "   ")).toBe(false);
    expect(hasRequiredPrompt(false, "do the thing")).toBe(true);
  });

  it("needs no input at all when the pipeline sources its own work", () => {
    expect(hasRequiredPrompt(true, "")).toBe(true);
  });
});

describe("triggerInputRejectReason (#161)", () => {
  const base = {
    mode: "trigger" as const,
    selectedPipeline: pipeline(),
    promptOptional: false,
    guardCommand: "",
    input: "",
  };

  it("mirrors the server reject for a prompt-required pipeline with no guard and no template", () => {
    expect(triggerInputRejectReason(base)).toMatch(/requires a prompt/);
  });

  it("says nothing in run mode", () => {
    expect(triggerInputRejectReason({ ...base, mode: "run" })).toBeNull();
  });

  it("says nothing before a pipeline is chosen", () => {
    expect(triggerInputRejectReason({ ...base, selectedPipeline: undefined })).toBeNull();
  });

  it("is resolved by a guard command, whose stdout becomes the input", () => {
    expect(triggerInputRejectReason({ ...base, guardCommand: "gh issue list" })).toBeNull();
  });

  it("is resolved by an input template", () => {
    expect(triggerInputRejectReason({ ...base, input: "audit the codebase" })).toBeNull();
  });

  it("is resolved by a prompt-not-required pipeline", () => {
    expect(triggerInputRejectReason({ ...base, promptOptional: true })).toBeNull();
  });

  // Blanks are not input: the server resolves the same way, so the modal must not
  // pre-approve a fire it would reject.
  it("treats a whitespace-only guard and template as absent", () => {
    expect(triggerInputRejectReason({ ...base, guardCommand: "  ", input: " \n " })).toMatch(
      /requires a prompt/,
    );
  });
});

describe("canLaunch", () => {
  const ok = {
    repoValid: true,
    selectedPipeline: pipeline(),
    hasRequiredPrompt: true,
    missingProfile: false,
    sandboxDoomed: false,
  };

  it("allows a launch once repo, pipeline and prompt are settled", () => {
    expect(canLaunch(ok)).toBe(true);
  });

  it("refuses an unvalidated repo (#470: `null` is not a verdict)", () => {
    expect(canLaunch({ ...ok, repoValid: null })).toBe(false);
    expect(canLaunch({ ...ok, repoValid: false })).toBe(false);
  });

  it("refuses without a pipeline or without the required prompt", () => {
    expect(canLaunch({ ...ok, selectedPipeline: undefined })).toBe(false);
    expect(canLaunch({ ...ok, hasRequiredPrompt: false })).toBe(false);
  });

  // ADR-0031 §7: neither a vanished profile nor an unavailable Docker is silently
  // demoted — both refuse the launch instead.
  it("refuses a vanished profile and a Docker-doomed sandbox", () => {
    expect(canLaunch({ ...ok, missingProfile: true })).toBe(false);
    expect(canLaunch({ ...ok, sandboxDoomed: true })).toBe(false);
  });
});

describe("canCreateTrigger", () => {
  const ok = {
    repoValid: true,
    selectedPipeline: pipeline(),
    triggerName: "Nightly",
    resolvedCron: "0 * * * *",
    triggerInputRejectReason: null,
    missingProfile: false,
  };

  it("allows a create once name, pipeline, repo and cron are settled", () => {
    expect(canCreateTrigger(ok)).toBe(true);
  });

  it("refuses a blank name", () => {
    expect(canCreateTrigger({ ...ok, triggerName: "   " })).toBe(false);
  });

  it("refuses an unresolvable schedule", () => {
    expect(canCreateTrigger({ ...ok, resolvedCron: "" })).toBe(false);
  });

  it("refuses an unvalidated repo or a missing pipeline", () => {
    expect(canCreateTrigger({ ...ok, repoValid: null })).toBe(false);
    expect(canCreateTrigger({ ...ok, selectedPipeline: undefined })).toBe(false);
  });

  it("refuses the fire the server would reject", () => {
    expect(canCreateTrigger({ ...ok, triggerInputRejectReason: "…" })).toBe(false);
  });

  // #432: re-saving a Trigger whose profile vanished would PATCH `sandbox: null` — a
  // silent fallback to the instance default.
  it("refuses to re-save a trigger pointing at a vanished profile", () => {
    expect(canCreateTrigger({ ...ok, missingProfile: true })).toBe(false);
  });
});

describe("sandboxState (#410/#432/#452)", () => {
  it("asserts nothing while settings are unknown", () => {
    const state = sandboxState({ settings: null, sandbox: "", mode: "run" });
    expect(state).toMatchObject({
      dockerUnavailable: false,
      sandboxProfiles: [],
      missingProfile: false,
      instanceDefaultSandbox: null,
      effectiveSandbox: null,
      sandboxDoomed: false,
      inheritedDefaultReason: null,
    });
  });

  it("serves the daemon's profile list as the options", () => {
    const state = sandboxState({ settings: settings(), sandbox: "", mode: "run" });
    expect(state.sandboxProfiles.map((p) => p.name)).toEqual(["full", "minimal"]);
  });

  it("resolves the inherit sentinel to the instance default, and a pick to itself", () => {
    expect(
      sandboxState({ settings: settings(defaultIs("full")), sandbox: "", mode: "run" })
        .effectiveSandbox,
    ).toBe("full");
    expect(
      sandboxState({ settings: settings(defaultIs("full")), sandbox: "off", mode: "run" })
        .effectiveSandbox,
    ).toBe("off");
  });

  it("reads a null default tier as off", () => {
    const state = sandboxState({
      settings: settings({
        default_sandbox: { effective: null, source: "default", stored: null, env: null, default: null, reason: null },
      }),
      sandbox: "",
      mode: "run",
    });
    expect(state.instanceDefaultSandbox).toBe("off");
  });

  it("greys the profiles and names why once Docker is known unavailable", () => {
    const state = sandboxState({
      settings: settings({
        sandbox_docker: { available: false, reason: "Docker daemon unreachable", checked_at: "x" },
      }),
      sandbox: "",
      mode: "run",
    });
    expect(state.dockerUnavailable).toBe(true);
    expect(state.sandboxReason).toBe("Docker daemon unreachable");
  });

  /**
   * #452: the Docker clamp says NO instead of answering `off` on the user's behalf — the
   * inherited default counts, because that is what the Run will actually get.
   */
  it("dooms a run whose inherited default needs an unavailable Docker", () => {
    const dockerDown = settings({
      ...defaultIs("minimal"),
      sandbox_docker: { available: false, reason: "Docker daemon unreachable", checked_at: "x" },
    });
    expect(sandboxState({ settings: dockerDown, sandbox: "", mode: "run" }).sandboxDoomed).toBe(true);
    // Demoting to `off` is the user's call, and it unblocks.
    expect(sandboxState({ settings: dockerDown, sandbox: "off", mode: "run" }).sandboxDoomed).toBe(
      false,
    );
    // A Trigger resolves its sandbox when it FIRES: today's probe says nothing about it.
    expect(sandboxState({ settings: dockerDown, sandbox: "", mode: "trigger" }).sandboxDoomed).toBe(
      false,
    );
  });

  it("does not doom a run when Docker is available", () => {
    expect(
      sandboxState({ settings: settings(defaultIs("full")), sandbox: "full", mode: "run" })
        .sandboxDoomed,
    ).toBe(false);
  });

  // THE PHANTOM-PROFILE RULE: a dangling name is tombstoned, never rewritten.
  it("flags a value that names no served profile", () => {
    expect(
      sandboxState({ settings: settings(), sandbox: "full-no-mcp", mode: "trigger" })
        .missingProfile,
    ).toBe(true);
  });

  it("never tombstones off or the inherit sentinel", () => {
    expect(sandboxState({ settings: settings(), sandbox: "off", mode: "run" }).missingProfile).toBe(
      false,
    );
    expect(sandboxState({ settings: settings(), sandbox: "", mode: "run" }).missingProfile).toBe(
      false,
    );
  });

  it("cannot tombstone before the profile list has landed", () => {
    expect(
      sandboxState({ settings: null, sandbox: "full-no-mcp", mode: "run" }).missingProfile,
    ).toBe(false);
  });

  it("surfaces a dangling instance default only while inheriting it", () => {
    const dangling = settings(defaultIs("deleted-profile", "No staging profile named `deleted-profile`"));
    expect(
      sandboxState({ settings: dangling, sandbox: "", mode: "run" }).inheritedDefaultReason,
    ).toMatch(/deleted-profile/);
    // An explicit pick does not inherit, so the default's problem is not the user's.
    expect(
      sandboxState({ settings: dangling, sandbox: "off", mode: "run" }).inheritedDefaultReason,
    ).toBeNull();
  });
});

describe("buildRunPayload", () => {
  const fields = {
    selectedPipeline: pipeline({ id: "p1", name: "Auditor" }),
    input: "  implement feature X  ",
    variables: { max_iter: 5 },
    targetRepo: "  /home/user/project  ",
    sourceBranch: "dev",
    autoName: false,
    runName: "  Fix bug  ",
    sandbox: "full",
    images: [new File(["png"], "design.png", { type: "image/png" })],
  };

  it("posts the trimmed form values", () => {
    expect(buildRunPayload(fields)).toMatchObject({
      pipeline: "Auditor",
      pipeline_id: "p1",
      input: "implement feature X",
      variables: { max_iter: 5 },
      target_repo: "/home/user/project",
      source_branch: "dev",
      name: "Fix bug",
      auto_name: false,
      sandbox: "full",
    });
    expect(buildRunPayload(fields).images).toHaveLength(1);
  });

  // #338: the box wins over whatever is left in the name field — the daemon never has to
  // guess from `name` presence.
  it("sends no name at all when the manager auto-names the Run", () => {
    expect(buildRunPayload({ ...fields, autoName: true })).toMatchObject({
      name: undefined,
      auto_name: true,
    });
  });

  it("omits a blank name, repo, branch and image list", () => {
    const payload = buildRunPayload({
      ...fields,
      runName: "   ",
      targetRepo: "  ",
      sourceBranch: "",
      images: [],
    });
    expect(payload.name).toBeUndefined();
    expect(payload.target_repo).toBeUndefined();
    expect(payload.source_branch).toBeUndefined();
    expect(payload.images).toBeUndefined();
  });

  /**
   * #452, the whole issue in one assertion: `""` means "the user did not choose", and only
   * an ABSENT `sandbox` lets the daemon apply `default_sandbox` — it reads a present `off`
   * as final.
   */
  it("omits the sandbox key for the inherit sentinel, and sends an explicit off", () => {
    expect(buildRunPayload({ ...fields, sandbox: "" }).sandbox).toBeUndefined();
    expect(buildRunPayload({ ...fields, sandbox: "off" }).sandbox).toBe("off");
  });
});

describe("buildTriggerCreatePayload / buildTriggerUpdatePayload", () => {
  const fields = {
    selectedPipeline: pipeline({ id: "p2", name: "Bugfixer" }),
    triggerName: "  Nightly audit  ",
    resolvedCron: "*/15 * * * *",
    input: "  audit the codebase  ",
    guardCommand: "  gh issue list  ",
    targetRepo: "  /home/user/project  ",
    sourceBranch: "dev",
    allowOverlap: true,
    maxConcurrent: " 2 ",
    sandbox: "minimal",
    autoName: true,
    variables: { max_iter: 5 },
  };

  it("creates with the trimmed form values", () => {
    expect(buildTriggerCreatePayload(fields)).toEqual({
      name: "Nightly audit",
      // #230: the current pipeline is always sent, so an unchanged edit is a no-op repoint.
      pipeline_id: "p2",
      cron: "*/15 * * * *",
      input_template: "audit the codebase",
      guard_command: "gh issue list",
      target_repo: "/home/user/project",
      source_branch: "dev",
      overlap_policy: "allow",
      max_concurrent: 2,
      sandbox: "minimal",
      auto_name: true,
      variables: { max_iter: 5 },
    });
  });

  it("patches the same values, with `null` where a create simply omits", () => {
    expect(buildTriggerUpdatePayload(fields)).toEqual({
      name: "Nightly audit",
      pipeline_id: "p2",
      cron: "*/15 * * * *",
      input_template: "audit the codebase",
      guard_command: "gh issue list",
      target_repo: "/home/user/project",
      // #465: a fixture with no secondaries patches `target_repos` to `null`, clearing
      // any stored list back to mono-repo (a create simply omits the key).
      target_repos: null,
      source_branch: "dev",
      overlap_policy: "allow",
      max_concurrent: 2,
      sandbox: "minimal",
      auto_name: true,
      variables: { max_iter: 5 },
    });
  });

  /**
   * The create/update asymmetry, which is the reason these are two functions: on a PATCH
   * `undefined` means "leave unchanged", so clearing a field has to be an explicit `null`.
   * An emptied guard CLEARS it (#162 `Some(None)`); a blank one is simply absent on create.
   */
  it("clears an emptied guard, repo, branch and cap on a patch, and omits them on a create", () => {
    const blanked = { ...fields, guardCommand: "  ", targetRepo: "  ", sourceBranch: "", allowOverlap: false };
    expect(buildTriggerUpdatePayload(blanked)).toMatchObject({
      guard_command: null,
      target_repo: null,
      source_branch: null,
      max_concurrent: null,
      overlap_policy: "skip",
    });
    expect(buildTriggerCreatePayload(blanked)).toMatchObject({
      guard_command: undefined,
      target_repo: undefined,
      source_branch: undefined,
      max_concurrent: undefined,
      overlap_policy: "skip",
    });
  });

  // #239: the cap only means something with overlap allowed, and a blank cap is unbounded.
  it("drops the cap when overlap is off or the field is blank", () => {
    expect(buildTriggerCreatePayload({ ...fields, allowOverlap: false }).max_concurrent).toBeUndefined();
    expect(buildTriggerCreatePayload({ ...fields, maxConcurrent: "  " }).max_concurrent).toBeUndefined();
    expect(buildTriggerUpdatePayload({ ...fields, maxConcurrent: "" }).max_concurrent).toBeNull();
  });

  // #410: unlike a Run, a Trigger has a real "inherit" state — so `""` is `null` here,
  // not an omission.
  it("sends null for the inherit sentinel on both paths", () => {
    expect(buildTriggerCreatePayload({ ...fields, sandbox: "" }).sandbox).toBeNull();
    expect(buildTriggerUpdatePayload({ ...fields, sandbox: "" }).sandbox).toBeNull();
  });

  // A create sends no input_template at all when blank; a patch clears it to "".
  it("differs on a blank input template: absent on create, emptied on patch", () => {
    expect(buildTriggerCreatePayload({ ...fields, input: "   " }).input_template).toBeUndefined();
    expect(buildTriggerUpdatePayload({ ...fields, input: "   " }).input_template).toBe("");
  });
});
