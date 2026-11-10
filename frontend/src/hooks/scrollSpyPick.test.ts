import { describe, expect, it } from "vitest";
import { pickSection } from "./scrollSpyPick";

const view = { top: 0, height: 600 };

describe("pickSection", () => {
  it("picks the heading nearest the top of the view", () => {
    const sections = [
      { id: "a", top: -400, bottom: -10 },
      { id: "b", top: 20, bottom: 500 },
      { id: "c", top: 500, bottom: 900 },
    ];
    expect(pickSection(sections, view, false)).toBe("b");
  });

  it("picks a long last section at the bottom, though its heading never reaches the top", () => {
    const sections = [
      { id: "a", top: -100, bottom: 200 },
      { id: "b", top: 200, bottom: 600 },
    ];
    expect(pickSection(sections, view, true)).toBe("b");
  });

  // FP of CORE-227 (C6): scrolled down to Tutorials, the column lit Stats
  // absorptions, a short last section under it. At the bottom, the section
  // that fills most of the view wins.
  it("keeps the section that fills the view at the bottom, over a short last one", () => {
    const sections = [
      { id: "version", top: -300, bottom: 40 },
      { id: "tutorials", top: 40, bottom: 480 },
      { id: "stats-absorptions", top: 480, bottom: 600 },
    ];
    expect(pickSection(sections, view, true)).toBe("tutorials");
  });
});
