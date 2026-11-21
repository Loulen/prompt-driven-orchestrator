import { describe, expect, it } from "vitest";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { HarnessCatalog, HarnessOption } from "../../harness/model/catalog";
import {
  agentTag,
  answerSeenKey,
  choiceCombination,
  choiceLabel,
  countdown,
  hasNewAnswer,
  idleDelay,
  initialChoice,
  installedCatalog,
  isRunnable,
  runnableProfiles,
} from "./supportAgent";

const harness = (name: string, installed: boolean): HarnessOption => ({
  name,
  installed,
  models: [],
  modelContexts: {},
  efforts: [],
  hasEffort: true,
  version: null,
});
const catalog: HarnessCatalog = {
  builtin: [harness("claude", true), harness("copilot", false)],
  descriptors: [harness("pi", true)],
};
const profile = (id: string, name: string, h: string, model?: string): AgentProfile =>
  ({ id, name, harness: h, model, created_at: "", updated_at: "" }) as AgentProfile;
const profiles = [profile("default", "Default", "claude"), profile("gpt", "Copilot GPT", "copilot", "gpt-5.1"), profile("deep", "Deep", "claude", "opus")];

describe("support agent choices", () => {
  it("leaves out the harnesses not installed and their profiles", () => {
    expect(installedCatalog(catalog).builtin.map((h) => h.name)).toEqual(["claude"]);
    expect(installedCatalog(catalog).descriptors.map((h) => h.name)).toEqual(["pi"]);
    expect(runnableProfiles(profiles, catalog).map((p) => p.id)).toEqual(["default", "deep"]);
    expect(isRunnable({ mode: "profile", profile_id: "gpt" }, profiles, catalog)).toBe(false);
    expect(isRunnable({ mode: "custom", harness: "pi" }, profiles, catalog)).toBe(true);
    expect(isRunnable({ mode: "inherit" }, profiles, catalog)).toBe(false);
  });

  it("opens on the last choice while it runs here, else the Default profile", () => {
    const deep = { mode: "profile", profile_id: "deep" } as const;
    expect(initialChoice(deep, profiles, catalog)).toEqual(deep);
    expect(initialChoice({ mode: "profile", profile_id: "gpt" }, profiles, catalog)).toEqual({ mode: "profile", profile_id: "default" });
    expect(initialChoice(null, [profile("gpt", "GPT", "copilot")], catalog)).toEqual({ mode: "custom", harness: "claude", model: null, effort: null });
    expect(initialChoice(null, [], { builtin: [harness("claude", false)], descriptors: [] })).toBeNull();
  });

  it("names a choice and reads its combination", () => {
    expect(choiceLabel({ mode: "profile", profile_id: "deep" }, profiles)).toBe("Deep");
    expect(choiceLabel({ mode: "custom", harness: "pi" }, profiles)).toBe("pi");
    expect(choiceCombination({ mode: "profile", profile_id: "deep" }, profiles)).toEqual({ harness: "claude", model: "opus", effort: null });
    expect(agentTag({ harness: "claude", model: null })).toBe("claude · default model");
    expect(agentTag({ harness: "claude", model: "opus" })).toBe("claude · opus");
  });
});

describe("the support agent's idleness and answers", () => {
  it("says the idle delay in minutes when whole, else in seconds", () => {
    expect(idleDelay(1800)).toBe("30 min");
    expect(idleDelay(60)).toBe("1 min");
    expect(idleDelay(90)).toBe("90 s");
    expect(idleDelay(1)).toBe("1 s");
  });

  it("counts down from 0:60 and never below 0:00", () => {
    expect(countdown(60_000)).toBe("0:60");
    expect(countdown(59_001)).toBe("0:60");
    expect(countdown(59_000)).toBe("0:59");
    expect(countdown(9_000)).toBe("0:09");
    expect(countdown(-5_000)).toBe("0:00");
    expect(countdown(90_000)).toBe("1:30");
  });

  it("an answer is new when written after the one seen at the minimize", () => {
    const status = { alive: true, last_output_at: "2027-01-15T08:00:10.000Z" };
    expect(hasNewAnswer(status, "2027-01-15T08:00:05.000Z")).toBe(true);
    expect(hasNewAnswer(status, "2027-01-15T08:00:10.000Z")).toBe(false);
    expect(hasNewAnswer(status, ""), "nothing written when minimized").toBe(true);
    expect(hasNewAnswer(status, null), "never minimized here").toBe(false);
    expect(hasNewAnswer({ ...status, alive: false }, "")).toBe(false);
    expect(hasNewAnswer({ alive: true, last_output_at: null }, "")).toBe(false);
  });

  it("ties what was seen to one opening of one agent", () => {
    const agent = { choice: { mode: "custom" as const, harness: "claude" }, label: "Custom", harness: "claude", model: null, effort: null, opened_at: "T1" };
    expect(answerSeenKey({ session: "pdo-support-operator", alive: true, agent })).toBe("pdo-support-operator@T1");
    expect(answerSeenKey({ session: "pdo-support-operator", alive: false, agent: null })).toBeNull();
  });
});
