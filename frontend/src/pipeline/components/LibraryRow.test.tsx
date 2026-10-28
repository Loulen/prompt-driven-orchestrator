import type { ReactNode } from "react";
import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent, within } from "@testing-library/react";
import LibraryRow from "./LibraryRow";

function renderRow(
  over: {
    name?: string;
    nodeCount?: number;
    modified?: string | null;
    selected?: boolean;
    checked?: boolean;
    showDuplicate?: boolean;
    onOpen?: () => void;
    onDuplicate?: () => void;
    onRenameStart?: () => void;
    renaming?: boolean;
    renameValue?: string;
    onRenameValueChange?: (value: string) => void;
    onRenameCommit?: () => void;
    onRenameCancel?: () => void;
    onDelete?: () => void;
    onToggleSelect?: () => void;
    deleteTitle?: string;
    invalid?: string | null;
    indicator?: ReactNode;
    version?: number;
  } = {},
) {
  const props = {
    name: "fixture",
    nodeCount: 3,
    showDuplicate: false,
    onOpen: () => {},
    onDelete: () => {},
    deleteTitle: "Delete pipeline",
    ...over,
  };
  return render(<LibraryRow {...props} testId="row" />);
}

describe("LibraryRow, two lines", () => {
  it("opens on a click and on Enter", () => {
    const onOpen = vi.fn();
    renderRow({ onOpen });
    const row = screen.getByTestId("row");
    expect(row).toHaveAttribute("role", "button");
    fireEvent.click(row);
    fireEvent.keyDown(row, { key: "Enter" });
    expect(onOpen).toHaveBeenCalledTimes(2);
  });

  it("puts the name and its validation on line 1, the nodes, version and edit date on line 2", () => {
    renderRow({
      name: "planner",
      nodeCount: 7,
      version: 8,
      modified: "2026-10-25T12:00:00Z",
      indicator: <span>ind</span>,
    });
    expect(screen.getByTestId("library-row-name")).toHaveTextContent("planner");
    const meta = screen.getByTestId("library-row-meta");
    expect(meta).toHaveTextContent("7n");
    expect(within(meta).getByTestId("library-row-version")).toHaveTextContent("v8");
    expect(within(meta).getByTestId("library-row-edited")).toHaveTextContent("25 Oct");
    expect(meta).not.toContainElement(screen.getByText("ind"));
  });

  it("marks an invalid pipeline with its lint, and a valid one with nothing", () => {
    const { unmount } = renderRow();
    expect(screen.queryByTestId("library-row-lint")).toBeNull();
    expect(screen.getByTestId("row")).not.toHaveAttribute("data-invalid");
    unmount();

    renderRow({ invalid: "invalid YAML: line 3" });
    expect(screen.getByTestId("library-row-lint")).toHaveAccessibleName("Invalid: invalid YAML: line 3");
    expect(screen.getByTestId("row")).toHaveAttribute("data-invalid", "true");
  });

  it("tells the open tab and the ticked row apart", () => {
    const { unmount } = renderRow({ selected: true });
    expect(screen.getByTestId("row")).toHaveAttribute("data-open", "true");
    expect(screen.getByTestId("row")).not.toHaveAttribute("data-checked");
    unmount();

    renderRow({ checked: true, onToggleSelect: () => {} });
    expect(screen.getByTestId("row")).toHaveAttribute("data-checked", "true");
    expect(screen.getByRole("checkbox", { name: "Deselect fixture" })).toHaveAttribute("aria-checked", "true");
  });

  it("ticks from its lead icon without opening", () => {
    const onOpen = vi.fn();
    const onToggleSelect = vi.fn();
    renderRow({ onOpen, onToggleSelect });
    fireEvent.click(screen.getByRole("checkbox", { name: "Select fixture" }));
    expect(onToggleSelect).toHaveBeenCalledTimes(1);
    expect(onOpen).not.toHaveBeenCalled();
  });
});

describe("LibraryRow hover bar", () => {
  it("deletes from its trash, named by the delete title, without opening the row", () => {
    const onOpen = vi.fn();
    const onDelete = vi.fn();
    renderRow({ onOpen, onDelete, deleteTitle: "Remove from library" });

    fireEvent.click(screen.getByRole("button", { name: "Remove from library" }));
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("duplicates only when it offers it, without opening the row", () => {
    const { unmount } = renderRow({ showDuplicate: false, onDuplicate: () => {} });
    expect(screen.queryByTestId("library-duplicate-button")).not.toBeInTheDocument();
    unmount();

    const onOpen = vi.fn();
    const onDuplicate = vi.fn();
    renderRow({ showDuplicate: true, onDuplicate, onOpen });
    fireEvent.click(screen.getByTestId("library-duplicate-button"));
    expect(onDuplicate).toHaveBeenCalledTimes(1);
    expect(onOpen).not.toHaveBeenCalled();
  });
});

// #774 — inline rename: the pencil (or F2) flips the row's name into an input.
// Enter/blur commits, Escape cancels; the pencil never opens the row.
describe("LibraryRow inline rename", () => {
  it("offers a rename only when onRenameStart is wired", () => {
    const { unmount } = renderRow();
    expect(screen.queryByTestId("library-rename-button")).not.toBeInTheDocument();
    unmount();

    renderRow({ onRenameStart: () => {} });
    expect(screen.getByTestId("library-rename-button")).toBeInTheDocument();
  });

  it("starts the rename from the pencil or F2, without opening the row", () => {
    const onOpen = vi.fn();
    const onRenameStart = vi.fn();
    renderRow({ onOpen, onRenameStart });

    fireEvent.click(screen.getByTestId("library-rename-button"));
    fireEvent.keyDown(screen.getByTestId("row"), { key: "F2" });
    expect(onRenameStart).toHaveBeenCalledTimes(2);
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("renders an inline input while renaming, prefilled, the hover bar gone", () => {
    renderRow({ renaming: true, renameValue: "fixture" });
    expect(screen.getByTestId("library-rename-input")).toHaveValue("fixture");
    expect(screen.queryByTestId("library-row-actions")).not.toBeInTheDocument();
    expect(screen.queryByTestId("library-row-name")).not.toBeInTheDocument();
  });

  it("commits on Enter and on blur, cancels on Escape", () => {
    const onRenameCommit = vi.fn();
    const onRenameCancel = vi.fn();
    renderRow({ renaming: true, renameValue: "renamed", onRenameCommit, onRenameCancel });
    const input = screen.getByTestId("library-rename-input");
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.blur(input);
    expect(onRenameCommit).toHaveBeenCalledTimes(2);
    fireEvent.keyDown(input, { key: "Escape" });
    expect(onRenameCancel).toHaveBeenCalledTimes(1);
  });

  it("reports typed value changes, and a click in the input does not open the row", () => {
    const onOpen = vi.fn();
    const onRenameValueChange = vi.fn();
    renderRow({ onOpen, renaming: true, renameValue: "ren", onRenameValueChange });
    const input = screen.getByTestId("library-rename-input");
    fireEvent.change(input, { target: { value: "renamed" } });
    fireEvent.click(input);
    expect(onRenameValueChange).toHaveBeenCalledWith("renamed");
    expect(onOpen).not.toHaveBeenCalled();
  });
});

describe("LibraryRow shared draft (ADR-0088)", () => {
  it("shows who has the draft open and its changes", () => {
    render(
      <LibraryRow
        name="flow"
        nodeCount={5}
        showDuplicate={false}
        onOpen={() => {}}
        onDelete={() => {}}
        deleteTitle="Delete pipeline"
        draft={{
          changes: 3,
          nodes: 6,
          participants: [
            {
              connection: "4",
              actor: { kind: "local_operator" },
              display: null,
              color: 1,
              can_write: true,
              assistant: false,
              selection: null,
              typing: null,
            },
          ],
        }}
      />,
    );
    expect(screen.getByTestId("library-row-draft")).toHaveTextContent("draft: 3 changes");
    expect(screen.getByTestId("library-row-meta")).toHaveTextContent("6n");
    expect(screen.getAllByTestId("draft-participant")).toHaveLength(1);
  });

  it("shows nothing of a draft that is the file", () => {
    render(
      <LibraryRow
        name="flow"
        nodeCount={5}
        showDuplicate={false}
        onOpen={() => {}}
        onDelete={() => {}}
        deleteTitle="Delete"
        draft={{ changes: 0, nodes: 5, participants: [] }}
      />,
    );
    expect(screen.getByTestId("library-row-meta")).toHaveTextContent("5n");
    expect(screen.queryByTestId("library-row-draft")).not.toBeInTheDocument();
    expect(screen.queryByTestId("draft-participant")).not.toBeInTheDocument();
  });
});
