import { describe, expect, it } from "vitest";
import { GITHUB_DESTINATION } from "./bugReport";
import { destinationsOf, initialDestination, LAST_USED, opensInBrowser, reportBody, wireFields } from "./bugReportDestinations";

const ANSWER = {
  destinations: [
    { id: "desk/inbox", label: "Inbox", accepts_prompts: true, icon: "/ext/desk/inbox.svg", extension: "desk", extension_name: "Desk" },
    { id: "desk/board", label: "Board", accepts_prompts: false, icon: null, extension: "desk", extension_name: "Desk" },
  ],
  last: "desk/board",
};

describe("the destinations of Send to", () => {
  it("lists the core's first, then the extensions' under their name, the last choice marked", () => {
    const list = destinationsOf(ANSWER);
    expect(list.map((d) => d.id)).toEqual(["github", "desk/inbox", "desk/board"]);
    expect(list[0]).toBe(GITHUB_DESTINATION);
    expect(list[1]).toMatchObject({
      label: "Inbox",
      acceptsPrompts: true,
      group: "Desk",
      hint: undefined,
      sendLabel: "Send to Inbox",
      icon: "/ext/desk/inbox.svg",
      extension: "desk",
    });
    expect(list[2].hint).toBe(LAST_USED);
    expect(destinationsOf(null)).toEqual([GITHUB_DESTINATION]);
  });

  it("starts on the last choice while it is listed, else on the core's", () => {
    const list = destinationsOf(ANSWER);
    expect(initialDestination(list, "desk/board")).toBe("desk/board");
    expect(initialDestination(list, "github")).toBe("github");
    expect(initialDestination(list, "gone/away")).toBe("github");
    expect(initialDestination(list, null)).toBe("github");
  });

  it("opens the core's in the browser, sends an extension's through the daemon", () => {
    const [github, inbox] = destinationsOf(ANSWER);
    expect(opensInBrowser(github)).toBe(true);
    expect(opensInBrowser(inbox)).toBe(false);
  });

  it("carries every section, null when absent, and the screenshot in base64", () => {
    expect(wireFields({ description: "x", prompts: "p" })).toEqual({
      description: "x",
      version: null,
      os: null,
      harnesses: null,
      run: null,
      pipeline: null,
      prompts: "p",
    });
    const [, inbox] = destinationsOf(ANSWER);
    expect(reportBody(inbox, "### Description\nx", { description: "x" }, null)).toEqual({
      destination: "desk/inbox",
      text: "### Description\nx",
      fields: wireFields({ description: "x" }),
    });
    expect(reportBody(inbox, "t", {}, { mediaType: "image/png", base64: "cG5n" }).screenshot).toEqual({
      media_type: "image/png",
      data: "cG5n",
    });
  });
});
