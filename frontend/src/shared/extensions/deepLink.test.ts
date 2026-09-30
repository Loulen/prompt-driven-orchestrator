import { describe, expect, it } from "vitest";
import { formatDeepLink, isDeepLink, parseDeepLink } from "./deepLink";

describe("a deep link", () => {
  it("names the extension, its page location and the path", () => {
    expect(parseDeepLink("#/ext/pm/topbar/end/backlog/metier")).toEqual({
      extension: "pm",
      location: "topbar/end",
      path: "backlog/metier",
    });
    expect(parseDeepLink("http://localhost:6280/#/ext/test/topbar/start")).toEqual({
      extension: "test",
      location: "topbar/start",
      path: "",
    });
  });

  it("names no location when what follows the id is not a page location", () => {
    expect(parseDeepLink("#/ext/audit/journal")).toEqual({ extension: "audit", location: null, path: "journal" });
    expect(parseDeepLink("#/ext/pm/topbar/endless")).toEqual({
      extension: "pm",
      location: null,
      path: "topbar/endless",
    });
    expect(parseDeepLink("#/runs")).toBeNull();
    expect(parseDeepLink("#/ext/")).toBeNull();
  });

  it("round-trips through its written form", () => {
    const link = formatDeepLink("pm", "topbar/end", "/page/Doc é");
    expect(link).toBe("#/ext/pm/topbar/end/page/Doc%20%C3%A9");
    expect(parseDeepLink(link)?.path).toBe("page/Doc é");
    expect(isDeepLink(link)).toBe(true);
    expect(isDeepLink("/#/ext/pm/topbar/end")).toBe(true);
    expect(isDeepLink("https://example.com/#/ext/pm")).toBe(false);
  });
});
