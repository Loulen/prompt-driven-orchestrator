import { describe, expect, it } from "vitest";
import {
  formatPageLink,
  guideLinkLabel,
  guideTarget,
  guidedPageUrl,
  isPdoTarget,
  pageUrl,
  parsePageLink,
} from "./mountedPage";

describe("the link toward a mounted page", () => {
  it("names the page, however it is written", () => {
    expect(parsePageLink("#/pages/core-design")).toBe("core-design");
    expect(parsePageLink("https://pdo.example.net/#/pages/demo_1/")).toBe("demo_1");
    expect(parsePageLink(formatPageLink("demo"))).toBe("demo");
    expect(parsePageLink("#/pages/")).toBeNull();
    expect(parsePageLink("#/pages/Not A Name")).toBeNull();
    expect(parsePageLink("#/runs/r1")).toBeNull();
  });

  it("and the addresses `pdo page mount` prints", () => {
    expect(guidedPageUrl("https://pdo.example.net/", "demo")).toBe("https://pdo.example.net/#/pages/demo");
    expect(pageUrl("https://pdo.example.net", "demo")).toBe("https://pdo.example.net/pages/demo/");
    expect(pageUrl("", "demo")).toBe("/pages/demo/");
  });
});

describe("a guide entry's link", () => {
  const runName = (id: string) => (id === "r1" ? "Add search filter" : null);

  it("leads into PDO: a Run, a node, an artifact, a Review file, an extension's page", () => {
    const run = guideTarget("#/runs/r1");
    expect(run).toEqual({ kind: "run", runId: "r1" });
    expect(guideLinkLabel(run, runName)).toEqual({ kind: "Run", where: "Add search filter" });
    const node = guideTarget("#/runs/r1/nodes/designer");
    expect(node).toEqual({ kind: "node", runId: "r1", nodeId: "designer", iter: null });
    expect(guideTarget("#/runs/r1/nodes/designer/iter/2")).toEqual({ kind: "node", runId: "r1", nodeId: "designer", iter: 2 });
    expect(guideLinkLabel(node, runName)).toEqual({ kind: "Run", where: "Add search filter › designer" });
    const artifact = guideTarget("#/runs/r2/nodes/designer/artifacts/before");
    expect(artifact).toEqual({ kind: "artifact", runId: "r2", nodeId: "designer", port: "before" });
    expect(guideLinkLabel(artifact, runName).kind).toBe("Artifact");
    const review = guideTarget("/runs/r1/review?file=frontend%2Fproto%2F");
    expect(review).toMatchObject({ kind: "review", runId: "r1", file: "frontend/proto/" });
    expect(guideLinkLabel(review, runName)).toEqual({ kind: "Review", where: "frontend/proto/" });
    expect(guideLinkLabel(guideTarget("/runs/r9/review"), runName)).toEqual({ kind: "Review", where: "r9" });
    expect(guideTarget("#/ext/demo/topbar/end").kind).toBe("extension");
    for (const link of ["#/runs/r1", "/runs/r1/review", "#/ext/demo/topbar/end"]) {
      expect(isPdoTarget(guideTarget(link))).toBe(true);
    }
  });

  it("or into the page: an anchor, a relative path", () => {
    const anchor = guideTarget("#88-snap:A");
    expect(anchor).toEqual({ kind: "anchor", anchor: "88-snap:A" });
    expect(guideLinkLabel(anchor, runName)).toEqual({ kind: "#", where: "#88-snap:A" });
    expect(guideTarget("sub/b.html#x")).toEqual({ kind: "path", path: "sub/b.html#x" });
    expect(isPdoTarget(anchor)).toBe(false);
    expect(isPdoTarget(guideTarget("index.html"))).toBe(false);
  });
});
