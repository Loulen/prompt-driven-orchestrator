import { describe, expect, it } from "vitest";
import { harnessColor } from "./color";
import { harnessCatalog, findHarnessOption, effortOffer, modelCount, probedAgo } from "./catalog";
import type { HarnessOption } from "./catalog";

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

describe("refresh models: the catalogue's size and age", () => {
  const now = new Date("2026-10-19T10:00:00.000Z");

  it("counts the models", () => {
    expect(modelCount(0)).toBe("no model listed");
    expect(modelCount(1)).toBe("1 model");
    expect(modelCount(23)).toBe("23 models");
  });

  it("says when the catalogue was probed", () => {
    expect(probedAgo("2026-10-19T09:59:30.000Z", now)).toBe("probed just now");
    expect(probedAgo("2026-10-19T09:48:00.000Z", now)).toBe("probed 12 min ago");
    expect(probedAgo("2026-10-19T07:00:00.000Z", now)).toBe("probed 3 h ago");
    expect(probedAgo("2026-10-18T10:00:00.000Z", now)).toBe("probed 1 day ago");
    expect(probedAgo("2026-10-16T10:00:00.000Z", now)).toBe("probed 3 days ago");
    expect(probedAgo(null, now)).toBe("not probed");
  });

  it("carries the time of the probe into the picker's option", () => {
    const catalog = harnessCatalog({
      path: null,
      names: ["claude"],
      harnesses: [
        { name: "claude", source: "builtin", installed: true, probed_at: "2026-10-19T09:00:00.000Z" },
        { name: "pi", source: "builtin", installed: false },
      ],
      rejected: [],
      reason: null,
    });
    expect(catalog.builtin[0].probedAt).toBe("2026-10-19T09:00:00.000Z");
    expect(catalog.builtin[1].probedAt).toBeNull();
  });
});
