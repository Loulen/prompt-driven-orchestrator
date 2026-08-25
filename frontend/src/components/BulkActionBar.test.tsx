import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import BulkActionBar from "./BulkActionBar";

describe("BulkActionBar", () => {
  it("shows the count, the domain note, and fires actions / clear", () => {
    const cleanup = vi.fn();
    const onClear = vi.fn();
    render(
      <BulkActionBar
        count={3}
        note="1 running will stop"
        actions={[{ key: "cleanup", label: "Cleanup", destructive: true, onClick: cleanup }]}
        onClear={onClear}
      />,
    );
    expect(screen.getByTestId("bulk-count")).toHaveTextContent("3 selected");
    expect(screen.getByTestId("bulk-note")).toHaveTextContent("1 running will stop");
    fireEvent.click(screen.getByTestId("bulk-action-cleanup"));
    expect(cleanup).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByTestId("bulk-clear"));
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("omits the note when none is given", () => {
    render(<BulkActionBar count={1} actions={[]} onClear={() => {}} />);
    expect(screen.queryByTestId("bulk-note")).not.toBeInTheDocument();
  });

  it("disables an action whose valid subset is empty", () => {
    const onClick = vi.fn();
    render(
      <BulkActionBar
        count={2}
        actions={[{ key: "retry", label: "Retry", disabled: true, onClick }]}
        onClear={() => {}}
      />,
    );
    const btn = screen.getByTestId("bulk-action-retry");
    expect(btn).toBeDisabled();
    fireEvent.click(btn);
    expect(onClick).not.toHaveBeenCalled();
  });
});
