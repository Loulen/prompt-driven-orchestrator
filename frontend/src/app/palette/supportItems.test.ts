import { describe, expect, it, vi } from "vitest";
import type { SupportStatusResponse } from "../../shared/generated/SupportStatusResponse";
import { buildSections } from "./paletteModel";
import { GROUP_ICONS } from "./paletteItems";
import { isDefaultRow, supportItems } from "./supportItems";

const live: SupportStatusResponse = {
  session: "pdo-support-operator",
  alive: true,
  agent: { choice: { mode: "profile", profile_id: "default" }, label: "Default", harness: "claude", model: "opus", effort: null, opened_at: "" },
  last_choice: null,
};
const closed: SupportStatusResponse = { ...live, alive: false, agent: null };

const gestures = () => ({ showDialog: vi.fn(), ask: vi.fn(), expand: vi.fn() });

describe("the Support group of the palette", () => {
  it("without an agent: open one, or ask one about the text", () => {
    const g = gestures();
    const empty = supportItems("", closed, g);
    expect(empty.map((i) => i.label)).toEqual(["Open support agent…"]);
    empty[0].run();
    expect(g.showDialog).toHaveBeenCalledWith();
    const asking = supportItems("which Runs failed?", null, g);
    expect(asking.map((i) => i.label)).toEqual(["Ask the support agent: which Runs failed?"]);
    asking[0].run();
    expect(g.showDialog).toHaveBeenCalledWith("which Runs failed?");
  });

  it("with an agent open: show it or ask it at once, or open another", () => {
    const g = gestures();
    const empty = supportItems("", live, g);
    expect(empty.map((i) => i.label)).toEqual(["Open support agent", "Open another support agent…"]);
    expect(empty[0].lead).toEqual({ kind: "harness", harness: "claude" });
    expect(empty[0].hint).toBe("claude · opus · open");
    empty[0].run();
    expect(g.expand).toHaveBeenCalled();
    const asking = supportItems("how many Pipelines?", live, g);
    expect(asking.map((i) => i.label)).toEqual(["Ask the open agent: how many Pipelines?", "Ask another agent: how many Pipelines?"]);
    asking[0].run();
    expect(g.ask).toHaveBeenCalledWith("how many Pipelines?");
    expect(g.showDialog).not.toHaveBeenCalled();
    asking[1].run();
    expect(g.showDialog).toHaveBeenCalledWith("how many Pipelines?");
  });

  it("never lets Enter type the query into the open agent before the arrows point at it", () => {
    const [askOpen, askAnother] = supportItems("run 1234", live, gestures());
    expect(isDefaultRow({ kind: "item", key: askOpen.id, item: askOpen, hits: [] })).toBe(false);
    expect(isDefaultRow({ kind: "item", key: askAnother.id, item: askAnother, hits: [] })).toBe(true);
  });

  it("closes the list, after the results, and never under `>`", () => {
    expect(supportItems(null, live, gestures())).toEqual([]);
    const run = { id: "run:r1", group: "runs", label: "why the deploy failed", lead: { kind: "icon", icon: GROUP_ICONS.runs }, run: () => {} } as const;
    const sections = buildSections({
      items: [run],
      selection: [],
      recent: [],
      extensions: [],
      query: "deploy",
      expanded: new Set(),
      icons: GROUP_ICONS,
      support: { icon: GROUP_ICONS.commands, items: supportItems("deploy", closed, gestures()) },
    });
    expect(sections.map((s) => s.key)).toEqual(["runs", "support"]);
    expect(sections[1].title).toBe("Support");
  });
});
