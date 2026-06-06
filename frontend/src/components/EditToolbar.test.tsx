import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, it, expect, vi, beforeEach } from "vitest";
import EditToolbar from "./EditToolbar";
import { TooltipProvider } from "./ui/tooltip";

describe("EditToolbar", () => {
  const onAddNode = vi.fn();
  const onLibraryDelete = vi.fn();

  beforeEach(() => {
    onAddNode.mockClear();
    onLibraryDelete.mockClear();
  });

  function renderToolbar() {
    return render(
      <TooltipProvider>
        <EditToolbar
          onAddNode={onAddNode}
          libraryEntries={[]}
          onLibraryDelete={onLibraryDelete}
        />
      </TooltipProvider>,
    );
  }

  it("renders the core icon buttons", () => {
    renderToolbar();
    expect(screen.getByTestId("toolbar-add")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-library")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-merge")).toBeInTheDocument();
    // The Switch node was removed (ADR-0011): conditional routing now lives on
    // the edge, authored via the edge detail panel (#147).
    expect(screen.queryByTestId("toolbar-switch")).toBeNull();
    // The legacy Loop node was removed (#171): loops are expressed as a
    // `loops:` region, created by drawing a cycle (#166) — not a toolbar add.
    expect(screen.queryByTestId("toolbar-loop")).toBeNull();
  });

  it("add button calls onAddNode with code-mutating", () => {
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-add"));
    expect(onAddNode).toHaveBeenCalledWith("code-mutating");
  });

  it("merge button calls onAddNode with merge", () => {
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-merge"));
    expect(onAddNode).toHaveBeenCalledWith("merge");
  });

  it("tooltips render the correct text on hover", async () => {
    const user = userEvent.setup();
    renderToolbar();

    await user.hover(screen.getByTestId("toolbar-add"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("New node");
    });

    fireEvent.pointerDown(screen.getByTestId("toolbar-add"));
    await waitFor(() => {
      expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument();
    });

    await user.hover(screen.getByTestId("toolbar-library"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Library");
    });

    fireEvent.pointerDown(screen.getByTestId("toolbar-library"));
    await waitFor(() => {
      expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument();
    });

    await user.hover(screen.getByTestId("toolbar-merge"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Merge node");
    });
  });
});
