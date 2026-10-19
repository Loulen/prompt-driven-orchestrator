import { beforeEach, describe, expect, it } from "vitest";
import type { PageMount } from "../../shared/generated/PageMount";
import { GUIDE_OPEN_KEY, useMountedPages } from "./mountedPages";

const mount = (name: string): PageMount => ({
  name,
  directory: `/srv/${name}`,
  run_id: null,
  node_id: null,
  created_at: "now",
  guide: null,
});

describe("the mounted pages' tabs", () => {
  beforeEach(() => {
    localStorage.clear();
    useMountedPages.getState().reset();
  });

  it("open once each, show the last opened, and give the canvas back when closed", () => {
    const { openPage, showPage, closePage } = useMountedPages.getState();
    openPage(mount("a"));
    openPage(mount("b"));
    openPage({ ...mount("a"), directory: "/elsewhere" });
    let state = useMountedPages.getState();
    expect(state.tabs.map((t) => t.name)).toEqual(["a", "b"]);
    expect(state.tabs[0].directory).toBe("/elsewhere");
    expect(state.active).toBe("a");
    showPage(null);
    expect(useMountedPages.getState().active).toBeNull();
    showPage("b");
    closePage("b");
    state = useMountedPages.getState();
    expect(state.tabs.map((t) => t.name)).toEqual(["a"]);
    expect(state.active).toBeNull();
  });

  it("remember the guide folded on this browser", () => {
    expect(useMountedPages.getState().guideOpen).toBe(true);
    useMountedPages.getState().setGuideOpen(false);
    expect(localStorage.getItem(GUIDE_OPEN_KEY)).toBe("closed");
    useMountedPages.getState().reset();
    expect(useMountedPages.getState().guideOpen).toBe(false);
  });

  it("number each request to move a page's frame", () => {
    const { goInPage } = useMountedPages.getState();
    goInPage("a", "#x");
    goInPage("a", "#x");
    expect(useMountedPages.getState().frameRequest).toEqual({ name: "a", link: "#x", seq: 2 });
  });
});
