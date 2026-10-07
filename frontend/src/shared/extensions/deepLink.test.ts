import { describe, expect, it } from "vitest";
import { formatDeepLink, formatRunLink, isDeepLink, isRunLink, parseDeepLink, parseRunLink } from "./deepLink";

describe("a deep link", () => {
  it("names the extension, its page location and the path", () => {
    expect(parseDeepLink("#/ext/demo/topbar/end/folder/drafts")).toEqual({
      extension: "demo",
      location: "topbar/end",
      path: "folder/drafts",
    });
    expect(parseDeepLink("http://localhost:6280/#/ext/test/topbar/start")).toEqual({
      extension: "test",
      location: "topbar/start",
      path: "",
    });
  });

  it("names no location when what follows the id is not a page location", () => {
    expect(parseDeepLink("#/ext/audit/journal")).toEqual({ extension: "audit", location: null, path: "journal" });
    expect(parseDeepLink("#/ext/demo/topbar/endless")).toEqual({
      extension: "demo",
      location: null,
      path: "topbar/endless",
    });
    expect(parseDeepLink("#/runs")).toBeNull();
    expect(parseDeepLink("#/ext/")).toBeNull();
  });

  it("round-trips through its written form", () => {
    const link = formatDeepLink("demo", "topbar/end", "/page/Doc é");
    expect(link).toBe("#/ext/demo/topbar/end/page/Doc%20%C3%A9");
    expect(parseDeepLink(link)?.path).toBe("page/Doc é");
    expect(isDeepLink(link)).toBe(true);
    expect(isDeepLink("/#/ext/demo/topbar/end")).toBe(true);
    expect(isDeepLink("https://example.com/#/ext/demo")).toBe(false);
  });
});

describe("a link toward a Run", () => {
  it("names the Run, however it is written", () => {
    expect(parseRunLink("#/runs/20261005-171614-2fb5c1b")).toBe("20261005-171614-2fb5c1b");
    expect(parseRunLink("http://localhost:5272/#/runs/r1/")).toBe("r1");
    expect(parseRunLink(formatRunLink("a b/c"))).toBeNull();
    expect(parseRunLink(formatRunLink("a b"))).toBe("a b");
    expect(parseRunLink("#/runs/")).toBeNull();
    expect(parseRunLink("#/runs/r1/review")).toBeNull();
    expect(parseRunLink("#/ext/demo/topbar/end")).toBeNull();
  });

  it("is a link of its own, not an extension's", () => {
    expect(isRunLink("#/runs/r1")).toBe(true);
    expect(isRunLink("/#/runs/r1")).toBe(true);
    expect(isRunLink("#/ext/demo")).toBe(false);
    expect(isDeepLink("#/runs/r1")).toBe(false);
    expect(parseDeepLink("#/runs/r1")).toBeNull();
  });
});

describe("a deep link toward a page of the Settings", () => {
  it("names the `settings/page` location, before the `settings` prefix", () => {
    expect(parseDeepLink("#/ext/test/settings/page/deep/inside")).toEqual({
      extension: "test",
      location: "settings/page",
      path: "deep/inside",
    });
    expect(parseDeepLink("#/ext/test/settings/page")?.location).toBe("settings/page");
    expect(parseDeepLink("#/ext/test/settings")?.location).toBeNull();
    expect(formatDeepLink("test", "settings/page", "a")).toBe("#/ext/test/settings/page/a");
  });
});
