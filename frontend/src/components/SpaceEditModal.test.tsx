import { describe, it, expect, vi, beforeEach } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import SpaceEditModal from "./SpaceEditModal";
import { createSpace, deleteSpace, updateSpace } from "../api";
import type { Space } from "../types";

vi.mock("../api", () => ({
  ApiError: class ApiError extends Error {},
  createSpace: vi.fn().mockResolvedValue({ id: "spc-n", name: "N", color: "green" }),
  updateSpace: vi.fn().mockResolvedValue({ id: "spc-l", name: "L", color: "sky" }),
  deleteSpace: vi.fn().mockResolvedValue(undefined),
}));

const loic: Space = { id: "spc-l", name: "Loïc", color: "blue" };
const ana: Space = { id: "spc-a", name: "Ana", color: "orange" };

beforeEach(() => vi.clearAllMocks());

describe("SpaceEditModal (#1041)", () => {
  it("creates an Espace with the first free colour pre-selected", async () => {
    const onSaved = vi.fn();
    const onClose = vi.fn();
    render(<SpaceEditModal space={null} spaces={[loic, ana]} onClose={onClose} onSaved={onSaved} />);
    expect(screen.getByTestId("space-color-green")).toHaveAttribute("aria-checked", "true");
    // Create is refused while the name is blank.
    expect(screen.getByTestId("space-edit-save")).toBeDisabled();
    fireEvent.change(screen.getByTestId("space-name-input"), { target: { value: "  Houss " } });
    fireEvent.click(screen.getByTestId("space-edit-save"));
    await waitFor(() => expect(createSpace).toHaveBeenCalledWith("Houss", "green"));
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();
  });

  it("renames and recolours an existing Espace", async () => {
    render(<SpaceEditModal space={loic} spaces={[loic]} onClose={vi.fn()} onSaved={vi.fn()} />);
    expect(screen.getByTestId("space-color-blue")).toHaveAttribute("aria-checked", "true");
    fireEvent.change(screen.getByTestId("space-name-input"), { target: { value: "Loïc B." } });
    fireEvent.click(screen.getByTestId("space-color-sky"));
    fireEvent.click(screen.getByTestId("space-edit-save"));
    await waitFor(() =>
      expect(updateSpace).toHaveBeenCalledWith("spc-l", { name: "Loïc B.", color: "sky" }),
    );
  });

  it("deletes only after a confirm that says the runs go back to No space", async () => {
    const onSaved = vi.fn();
    render(<SpaceEditModal space={loic} spaces={[loic]} onClose={vi.fn()} onSaved={onSaved} />);
    fireEvent.click(screen.getByTestId("space-delete"));
    expect(screen.getByTestId("space-delete-confirm")).toHaveTextContent(/move to No space/);
    expect(deleteSpace).not.toHaveBeenCalled();
    fireEvent.click(screen.getByTestId("space-delete-confirm-button"));
    await waitFor(() => expect(deleteSpace).toHaveBeenCalledWith("spc-l"));
    expect(onSaved).toHaveBeenCalled();
  });

  it("offers no Delete when creating", () => {
    render(<SpaceEditModal space={null} spaces={[]} onClose={vi.fn()} onSaved={vi.fn()} />);
    expect(screen.queryByTestId("space-delete")).not.toBeInTheDocument();
    expect(screen.getByTestId("space-color-blue")).toHaveAttribute("aria-checked", "true");
  });

  it("shows the daemon's refusal and stays open", async () => {
    vi.mocked(createSpace).mockRejectedValueOnce(new Error("space name is required"));
    const onClose = vi.fn();
    render(<SpaceEditModal space={null} spaces={[]} onClose={onClose} onSaved={vi.fn()} />);
    fireEvent.change(screen.getByTestId("space-name-input"), { target: { value: "x" } });
    fireEvent.click(screen.getByTestId("space-edit-save"));
    expect(await screen.findByTestId("space-edit-error")).toHaveTextContent("space name is required");
    expect(onClose).not.toHaveBeenCalled();
  });
});
