import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import LibraryRow from "./LibraryRow";

function renderRow(
  over: {
    name?: string;
    nodeCount?: number;
    modified?: string | null;
    selected?: boolean;
    showDuplicate?: boolean;
    onOpen?: () => void;
    onDuplicate?: () => void;
    onDelete?: () => void;
    deleteTitle?: string;
  } = {},
) {
  const props = {
    name: "fixture",
    nodeCount: 3,
    showDuplicate: false,
    onDelete: () => {},
    deleteTitle: "Delete pipeline",
    ...over,
  };
  return render(<LibraryRow {...props} testId="row" />);
}

describe("LibraryRow openable vs passive", () => {
  it("renders a <button> when onOpen is given, and clicking it opens", () => {
    const onOpen = vi.fn();
    renderRow({ onOpen });

    const row = screen.getByTestId("row");
    expect(row.tagName).toBe("BUTTON");
    fireEvent.click(row);
    expect(onOpen).toHaveBeenCalledTimes(1);
  });

  it("renders a passive <div> when onOpen is absent", () => {
    renderRow();

    const row = screen.getByTestId("row");
    expect(row.tagName).toBe("DIV");
    // Not a button by role either — there is nothing behind it to open.
    expect(row.getAttribute("role")).toBeNull();
  });

  it("shows the name and node count in both shapes", () => {
    const { unmount } = renderRow({ name: "planner", nodeCount: 7 });
    expect(screen.getByText("planner")).toBeInTheDocument();
    expect(screen.getByText("7 nodes")).toBeInTheDocument();
    unmount();

    renderRow({ name: "planner", nodeCount: 7, onOpen: () => {} });
    expect(screen.getByText("planner")).toBeInTheDocument();
    expect(screen.getByText("7 nodes")).toBeInTheDocument();
  });

  it("renders no data-testid attribute at all when testId is omitted", () => {
    // The /pipelines rows pass none; an empty `data-testid=""` would make them
    // matchable by an accidental query.
    const { container } = render(
      <LibraryRow
        name="fixture"
        nodeCount={3}
        showDuplicate={false}
        onDelete={() => {}}
        deleteTitle="Delete pipeline"
      />,
    );
    expect(container.firstElementChild!.hasAttribute("data-testid")).toBe(false);
  });
});

describe("LibraryRow instance metadata", () => {
  it("labels the useful last-edit metadata", () => {
    renderRow({ modified: "2026-08-26T12:00:00Z" });
    expect(screen.getByText(/^edited /)).toBeInTheDocument();
  });
});

describe("LibraryRow delete", () => {
  it("calls onDelete on the trash affordance, addressed by its title", () => {
    const onDelete = vi.fn();
    renderRow({ onDelete, deleteTitle: "Remove from library" });

    fireEvent.click(screen.getByRole("button", { name: "Remove from library" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
  });

  it("does not also open the row: the trash stops propagation", () => {
    const onOpen = vi.fn();
    const onDelete = vi.fn();
    renderRow({ onOpen, onDelete });

    // The affordance is a span nested inside the row <button>; without
    // stopPropagation the click would open the pipeline it is deleting.
    fireEvent.click(screen.getByRole("button", { name: "Delete pipeline" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onOpen).not.toHaveBeenCalled();
  });
});

describe("LibraryRow duplicate", () => {
  it("hides the duplicate affordance when showDuplicate is false", () => {
    renderRow({ showDuplicate: false, onDuplicate: () => {} });
    expect(screen.queryByTestId("library-duplicate-button")).not.toBeInTheDocument();
  });

  it("calls onDuplicate without opening the row", () => {
    const onOpen = vi.fn();
    const onDuplicate = vi.fn();
    renderRow({ showDuplicate: true, onDuplicate, onOpen });

    fireEvent.click(screen.getByTestId("library-duplicate-button"));
    expect(onDuplicate).toHaveBeenCalledTimes(1);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("survives a shown duplicate affordance with no handler wired", () => {
    renderRow({ showDuplicate: true });
    expect(() =>
      fireEvent.click(screen.getByTestId("library-duplicate-button")),
    ).not.toThrow();
  });
});

describe("LibraryRow selection", () => {
  it("marks the open editor tab with the selected background", () => {
    renderRow({ onOpen: () => {}, selected: true });
    const cls = screen.getByTestId("row").className;
    expect(cls).toContain("bg-bg-3 text-fg");
    // Not the unselected pair (whose `hover:bg-bg-3/50` also contains "bg-bg-3").
    expect(cls).not.toContain("hover:bg-bg-3/50");
  });

  it("leaves an unselected row on the hover-only background", () => {
    renderRow({ onOpen: () => {}, selected: false });
    const cls = screen.getByTestId("row").className;
    expect(cls).toContain("text-fg-2 hover:bg-bg-3/50");
  });

  it("defaults to unselected when `selected` is omitted", () => {
    renderRow({ onOpen: () => {} });
    expect(screen.getByTestId("row").className).toContain("hover:bg-bg-3/50");
  });
});
