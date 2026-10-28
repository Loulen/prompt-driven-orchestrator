/**
 * The loupe of a Run's canvas toolbar: between Review and the info group, it
 * opens and closes the search over the Run's transcripts. Absent on a
 * template's canvas.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../shared/generated/RunState";

vi.mock("./RunToolbarActions", () => ({ default: () => null }));

const { default: EditToolbar } = await import("./EditToolbar");
const { useRunSearchStore } = await import("../../run/store/runSearchStore");

function renderToolbar(run: RunState | null) {
  return render(
    <>
      <EditToolbar
        onAddNode={vi.fn()}
        onAddNote={vi.fn()}
        onAddNodeFromYaml={vi.fn()}
        libraryEntries={[]}
        onLibraryDelete={vi.fn()}
        readOnly
        reviewHref="#/review"
        onToggleInfo={vi.fn()}
        run={run}
      />
    </>,
  );
}

beforeEach(() => {
  useRunSearchStore.setState({ runId: null, query: "", result: null, status: "idle" });
});

describe("EditToolbar's loupe", () => {
  it("sits between Review and Info and toggles the Run's search", () => {
    renderToolbar({ run_id: "r1" } as RunState);
    const ids = [...screen.getByTestId("edit-toolbar").querySelectorAll("[data-testid^=toolbar-]")].map(
      (el) => el.getAttribute("data-testid"),
    );
    expect(ids.slice(-3)).toEqual(["toolbar-review", "toolbar-search", "toolbar-info"]);
    const loupe = screen.getByTestId("toolbar-search");
    expect(loupe.getAttribute("aria-pressed")).toBe("false");
    fireEvent.click(loupe);
    expect(useRunSearchStore.getState().runId).toBe("r1");
    expect(loupe.getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(loupe);
    expect(useRunSearchStore.getState().runId).toBeNull();
  });

  it("is absent on a template's canvas", () => {
    renderToolbar(null);
    expect(screen.queryByTestId("toolbar-search")).toBeNull();
  });
});
