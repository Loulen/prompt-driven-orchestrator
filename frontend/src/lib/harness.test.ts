import { describe, expect, it } from "vitest";
import { harnessColor, harnessCatalog, findHarnessOption, effortOffer } from "./harness";
import type { HarnessOption } from "./harness";

describe("harnessColor (#638)", () => {
  it("pins Copilot blue and Claude orange", () => {
    expect(harnessColor("copilot")).toBe("#58a6ff");
    expect(harnessColor("claude")).toBe("#f0883e");
  });

  it("assigns a stable colour to a future harness", () => {
    expect(harnessColor("future-harness")).toBe(harnessColor("future-harness"));
    expect(harnessColor("future-harness")).not.toBe(harnessColor("another-harness"));
  });
});

// #798: the effort offer for a (harness, model) pair. The regression fixture is
// the reported one: `union-alpha` supports ONLY `off`, while `GPT5.4` supports
// the full off..xhigh scale — two models of the SAME harness with disjoint
// support, so a stored effort can become unsupported by a model switch.
const UNION: HarnessOption = {
  name: "union",
  installed: true,
  models: ["union-alpha", "GPT5.4", "union-beta"],
  modelContexts: {},
  efforts: ["off", "low", "medium", "high"],
  modelEfforts: {
    "union-alpha": ["off"],
    "GPT5.4": ["off", "low", "medium", "high", "xhigh"],
  },
  hasEffort: true,
  version: "union 1.0",
};

describe("effortOffer (#798)", () => {
  it("uses pi's served provider-qualified model IDs without rewriting them", () => {
    const catalog = harnessCatalog({ path: "", names: ["pi"], rejected: [], reason: null, harnesses: [{
      name: "pi", source: "descriptor", installed: true,
      efforts: ["off", "minimal", "low", "medium", "high", "xhigh", "max"],
      model_efforts: {
        "openrouter/stealth/union-alpha": ["off"],
        "openrouter/openai/gpt-5.4": ["off", "low", "medium", "high", "xhigh"],
      },
    }] });
    const pi = findHarnessOption(catalog, "pi");
    expect(effortOffer(pi, "openrouter/stealth/union-alpha").levels).toEqual(["off"]);
    expect(effortOffer(pi, "openrouter/openai/gpt-5.4").levels).toEqual([
      "off", "low", "medium", "high", "xhigh",
    ]);
  });
  it("an exact model_efforts key is authoritative for that model", () => {
    expect(effortOffer(UNION, "union-alpha")).toEqual({
      levels: ["off"],
      authoritative: true,
    });
    expect(effortOffer(UNION, "GPT5.4")).toEqual({
      levels: ["off", "low", "medium", "high", "xhigh"],
      authoritative: true,
    });
  });

  it("a key present with [] is authoritative too — no offer for that model", () => {
    const none: HarnessOption = {
      ...UNION,
      modelEfforts: { "union-alpha": [] },
    };
    expect(effortOffer(none, "union-alpha")).toEqual({ levels: [], authoritative: true });
  });

  it("a missing key retains the harness's global efforts fallback (no exceptions)", () => {
    expect(effortOffer(UNION, "union-beta")).toEqual({
      levels: ["off", "low", "medium", "high"],
      authoritative: false,
    });
  });

  it("no model selected reads the harness's global offer", () => {
    expect(effortOffer(UNION, null)).toEqual({
      levels: ["off", "low", "medium", "high"],
      authoritative: false,
    });
    expect(effortOffer(UNION, undefined)).toEqual(effortOffer(UNION, null));
    expect(effortOffer(UNION, "")).toEqual(effortOffer(UNION, null));
  });

  it("an option without modelEfforts (pre-#798 daemon, hand-built option) falls back to the global offer", () => {
    const legacy: HarnessOption = { ...UNION, modelEfforts: undefined };
    expect(effortOffer(legacy, "union-alpha")).toEqual({
      levels: ["off", "low", "medium", "high"],
      authoritative: false,
    });
  });

  it("an unknown harness (absent from the catalogue) knows nothing — pass-through stands", () => {
    expect(effortOffer(undefined, "union-alpha")).toEqual({
      levels: [],
      authoritative: false,
    });
  });

  it("a harness whose binary enumerates no effort axis and names nothing for the model knows nothing", () => {
    const blind: HarnessOption = {
      ...UNION,
      efforts: [],
      modelEfforts: { "GPT5.4": ["off", "low"] },
    };
    // union-alpha has no key and the global offer is empty ⇒ unknown, not
    // "unsupported": the historical free-text pass-through, no warning.
    expect(effortOffer(blind, "union-alpha")).toEqual({ levels: [], authoritative: false });
    // …while the model the binary DID name stays authoritative, `[]` included.
    expect(effortOffer({ ...blind, modelEfforts: { "GPT5.4": [] } }, "GPT5.4")).toEqual({
      levels: [],
      authoritative: true,
    });
  });
});
