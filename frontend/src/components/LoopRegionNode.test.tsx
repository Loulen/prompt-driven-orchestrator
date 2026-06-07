import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { LoopRegionNode, type LoopRegionNodeData } from "./LoopRegionNode";
import { endRegion, bumpRegion } from "../api";

vi.mock("../api", () => ({
  endRegion: vi.fn().mockResolvedValue(undefined),
  bumpRegion: vi.fn().mockResolvedValue(undefined),
}));

const mockEndRegion = vi.mocked(endRegion);
const mockBumpRegion = vi.mocked(bumpRegion);

function renderRegion(data: Partial<LoopRegionNodeData>) {
  const full: LoopRegionNodeData = {
    regionId: "review_loop",
    kind: "bounded",
    counterText: "2/2",
    exhausted: false,
    runId: null,
    width: 320,
    height: 120,
    ...data,
  };
  // The xyflow NodeProps shape is large; the component only reads `data`.
  return render(
    <LoopRegionNode {...({ data: full } as Parameters<typeof LoopRegionNode>[0])} />,
  );
}

describe("LoopRegionNode — route from manager (#152)", () => {
  beforeEach(() => {
    mockEndRegion.mockClear();
    mockBumpRegion.mockClear();
  });

  it("offers no manager affordance while the region is not exhausted", () => {
    renderRegion({ exhausted: false, runId: "run-1" });
    expect(
      screen.queryByTestId("loop-region-route-from-manager"),
    ).toBeNull();
  });

  it("offers 'route from manager' on an exhausted-unrouted region of a live run", () => {
    renderRegion({ exhausted: true, runId: "run-1" });
    const btn = screen.getByTestId("loop-region-route-from-manager");
    expect(btn).toBeTruthy();
    expect(btn.textContent?.toLowerCase()).toContain("route from manager");
  });

  it("ends the region by id when the affordance is triggered", () => {
    renderRegion({ exhausted: true, runId: "run-1", regionId: "review_loop" });
    fireEvent.click(screen.getByTestId("loop-region-route-from-manager"));
    expect(mockEndRegion).toHaveBeenCalledWith("run-1", "review_loop");
  });

  it("shows no affordance with no live run id (template view)", () => {
    renderRegion({ exhausted: true, runId: null });
    expect(
      screen.queryByTestId("loop-region-route-from-manager"),
    ).toBeNull();
  });
});
