// CORE-83 — the reading guide of a mounted page, in the right pane: who wrote it,
// its numbered entries, a link into PDO handed to the app shell, a link into the
// page handed to the page's frame, the fold remembered on this browser.
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PageMount } from "../../shared/generated/PageMount";
import { GUIDE_OPEN_KEY, useMountedPages } from "../store/mountedPages";
import GuidePanel from "./GuidePanel";
import MountedPageView from "./MountedPageView";

const page: PageMount = {
  name: "design",
  directory: "/srv/design",
  run_id: "r1",
  node_id: "designer",
  created_at: "now",
  owner: null,
  project_id: null,
  guide: [
    { title: "Start with option A", text: "The chosen snap.", link: "#snap-a" },
    { title: "Where it plugs", text: "", link: "#/runs/r1/nodes/designer" },
    { title: "The code", text: "", link: "/runs/r1/review?file=src%2Fa.ts" },
    { title: "The other page", text: "", link: "sub/b.html" },
  ],
};

function navigation() {
  return {
    runName: (id: string) => (id === "r1" ? "Add search filter" : null),
    openInPdo: vi.fn(),
  };
}

describe("the reading guide", () => {
  beforeEach(() => {
    localStorage.clear();
    useMountedPages.getState().reset();
  });

  it("says who wrote it and numbers its entries", () => {
    render(<GuidePanel page={page} navigation={navigation()} />);
    expect(screen.getByText("Reading guide")).toBeInTheDocument();
    expect(screen.getByTestId("guide-count")).toHaveTextContent("4");
    expect(screen.getByTestId("guide-panel")).toHaveTextContent("designerAdd search filter");
    expect(screen.getByTitle("Mounted by the agent of node designer of « Add search filter »")).toBeInTheDocument();
    expect(screen.getByTestId("guide-entry-0")).toHaveTextContent("1Start with option AThe chosen snap.");
    expect(screen.getByTestId("guide-link-1")).toHaveTextContent("Run · Add search filter › designer");
    expect(screen.getByTestId("guide-link-1")).toHaveAttribute("data-kind", "pdo");
    expect(screen.getByTestId("guide-link-2")).toHaveTextContent("Review · src/a.ts");
    expect(screen.getByTestId("guide-link-0")).toHaveTextContent("#snap-a");
    expect(screen.getByTestId("guide-link-0")).toHaveAttribute("data-kind", "page");
  });

  it("hands a link into PDO to the app and a link into the page to its frame", () => {
    const nav = navigation();
    render(<GuidePanel page={page} navigation={nav} />);
    fireEvent.click(screen.getByTestId("guide-link-1"));
    expect(nav.openInPdo).toHaveBeenCalledWith({ kind: "node", runId: "r1", nodeId: "designer", iter: null });
    expect(screen.getByTestId("guide-entry-1")).toHaveAttribute("data-current", "true");
    fireEvent.click(screen.getByTestId("guide-link-0"));
    expect(nav.openInPdo).toHaveBeenCalledTimes(1);
    expect(useMountedPages.getState().frameRequest).toMatchObject({ name: "design", link: "#snap-a" });
  });

  it("folds into the page's rail, and the rail unfolds it", () => {
    useMountedPages.getState().openPage(page);
    render(<GuidePanel page={page} navigation={navigation()} />);
    fireEvent.click(screen.getByTestId("guide-fold"));
    expect(useMountedPages.getState().guideOpen).toBe(false);
    expect(localStorage.getItem(GUIDE_OPEN_KEY)).toBe("closed");

    render(<MountedPageView page={page} />);
    const rail = screen.getByTestId("guide-rail");
    expect(rail).toHaveTextContent("Guide · 4");
    fireEvent.click(rail);
    expect(useMountedPages.getState().guideOpen).toBe(true);
    expect(screen.queryByTestId("guide-rail")).toBeNull();
  });

  it("shows the page in a frame under /pages/<name>/, without a rail when it has no guide", () => {
    render(<MountedPageView page={{ ...page, guide: null }} />);
    expect(screen.getByTestId("mounted-page-frame")).toHaveAttribute("src", "/pages/design/");
    useMountedPages.getState().setGuideOpen(false);
    expect(screen.queryByTestId("guide-rail")).toBeNull();
  });

  it("moves the frame to a path of the page", () => {
    render(<MountedPageView page={page} />);
    useMountedPages.getState().goInPage("design", "sub/b.html");
    return vi.waitFor(() =>
      expect(screen.getByTestId("mounted-page-frame").getAttribute("src")).toBe("/pages/design/sub/b.html"),
    );
  });
});
