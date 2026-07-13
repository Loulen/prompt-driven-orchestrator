import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import PooledInputRow from "./PooledInputRow";
import type { PooledInput } from "../lib/derivePooledInputs";
import { TooltipProvider } from "./ui/tooltip";

const pooledReview: PooledInput = {
  name: "review",
  repeated: false,
  sources: [
    { nodeId: "sec", label: "security-reviewer", edgeIndex: 0 },
    { nodeId: "perf", label: "perf-reviewer", edgeIndex: 3 },
  ],
};

function renderRow(props: Parameters<typeof PooledInputRow>[0]) {
  return render(
    <TooltipProvider>
      <PooledInputRow {...props} />
    </TooltipProvider>,
  );
}

describe("PooledInputRow — per-source delete (#339)", () => {
  it("renders read-only (no ×) when onDeleteSource is absent — unchanged render", () => {
    renderRow({ input: pooledReview });

    expect(screen.getByTestId("pooled-input-review")).toHaveTextContent("security-reviewer");
    expect(screen.getByTestId("pooled-input-review")).toHaveTextContent("perf-reviewer");
    expect(screen.queryByTestId("pooled-input-review-delete-sec")).toBeNull();
    expect(screen.queryByTestId("pooled-input-review-delete-perf")).toBeNull();
  });

  it("renders one × per source and reports each source's own edgeIndex", () => {
    const onDeleteSource = vi.fn();
    renderRow({ input: pooledReview, onDeleteSource });

    fireEvent.click(screen.getByTestId("pooled-input-review-delete-sec"));
    expect(onDeleteSource).toHaveBeenCalledWith(0);

    fireEvent.click(screen.getByTestId("pooled-input-review-delete-perf"));
    expect(onDeleteSource).toHaveBeenCalledWith(3);
  });
});
