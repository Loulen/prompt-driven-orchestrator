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

  it("renders four icon buttons", () => {
    renderToolbar();
    expect(screen.getByTestId("toolbar-add")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-library")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-loop")).toBeInTheDocument();
    expect(screen.getByTestId("toolbar-switch")).toBeInTheDocument();
  });

  it("add button calls onAddNode with code-mutating", () => {
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-add"));
    expect(onAddNode).toHaveBeenCalledWith("code-mutating");
  });

  it("loop button calls onAddNode with loop", () => {
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-loop"));
    expect(onAddNode).toHaveBeenCalledWith("loop");
  });

  it("switch button calls onAddNode with switch", () => {
    renderToolbar();
    fireEvent.click(screen.getByTestId("toolbar-switch"));
    expect(onAddNode).toHaveBeenCalledWith("switch");
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

    await user.hover(screen.getByTestId("toolbar-loop"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Loop node");
    });

    fireEvent.pointerDown(screen.getByTestId("toolbar-loop"));
    await waitFor(() => {
      expect(screen.queryByTestId("tooltip-content")).not.toBeInTheDocument();
    });

    await user.hover(screen.getByTestId("toolbar-switch"));
    await waitFor(() => {
      expect(screen.getByTestId("tooltip-content")).toHaveTextContent("Switch node");
    });
  });
});
