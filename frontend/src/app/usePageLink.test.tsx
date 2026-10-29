// CORE-83 — the deep link toward a mounted page (`#/pages/<name>`, what
// `pdo page mount` prints for a page with a guide): the page opens in its tab,
// an unknown page is a toast, and the link leaves the address bar.
import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import TabBar from "../pipeline/components/TabBar";
import { useMountedPages } from "../platform/store/mountedPages";
import type { PageMount } from "../shared/generated/PageMount";
import { resetToasts } from "../shared/ui/toast";
import Toaster from "../shared/ui/Toaster";
import { usePageLink } from "./usePageLink";

const design: PageMount = {
  name: "design",
  directory: "/srv/design",
  run_id: null,
  node_id: null,
  created_at: "now",
  guide: [{ title: "Start", text: "", link: "#a" }],
};

function Shell() {
  usePageLink();
  return (
    <>
      <TabBar />
      <Toaster />
    </>
  );
}

beforeEach(() => {
  resetToasts();
  useMountedPages.getState().reset();
  window.history.replaceState(null, "", "/");
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => (url === "/pages" ? Response.json([design]) : new Response("", { status: 404 }))),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("#/pages/<name>", () => {
  it("opens the page in its tab with its actions, then leaves the address bar", async () => {
    window.history.replaceState(null, "", "/#/pages/design");
    render(<Shell />);
    expect(await screen.findByTestId("page-tab-design")).toHaveAttribute("data-active", "true");
    expect(useMountedPages.getState().active).toBe("design");
    expect(screen.getByTestId("page-copy-link")).toHaveAccessibleName("Copy link");
    expect(screen.getByTestId("page-open-alone")).toHaveAttribute("href", "/pages/design/");
    expect(window.location.hash).toBe("");
  });

  it("toward a page nobody mounted says so and opens nothing", async () => {
    window.history.replaceState(null, "", "/#/pages/ghost");
    render(<Shell />);
    expect(await screen.findByTestId("toast-error")).toHaveTextContent(
      "Page “ghost” is not mounted: this link can't be opened.",
    );
    expect(useMountedPages.getState().tabs).toEqual([]);
    await waitFor(() => expect(window.location.hash).toBe(""));
  });
});
