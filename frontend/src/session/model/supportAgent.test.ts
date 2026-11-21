import { describe, expect, it } from "vitest";
import type { AgentProfile } from "../../shared/generated/AgentProfile";
import type { HarnessCatalog, HarnessOption } from "../../harness/model/catalog";
import { agentTag, choiceCombination, choiceLabel, initialChoice, installedCatalog, isRunnable, runnableProfiles } from "./supportAgent";

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
