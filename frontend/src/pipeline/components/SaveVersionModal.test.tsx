import { beforeEach, describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { SaveResult } from "../api/pipelines";
import SaveVersionModal from "./SaveVersionModal";

const toast = vi.hoisted(() => vi.fn());
vi.mock("../../shared/ui/kit/toast", () => ({ toast }));

const V9: SaveResult = { ok: true, version: { number: 9, title: "v9" } };

function renderModal(title = "external edit", result: SaveResult | null = V9) {
  const onSave = vi.fn().mockResolvedValue(result ?? undefined);
  const onClose = vi.fn();
  render(
    <SaveVersionModal
      pipelineName="feature-with-review"
      last={{ number: 8, title }}
      onSave={onSave}
      onClose={onClose}
    />,
  );
  return { onSave, onClose };
}

const choice = (name: string) => screen.getByRole("radio", { name });
const confirm = () => screen.getByTestId("save-version-confirm");
const titleInput = () => screen.getByTestId("save-version-title-input") as HTMLInputElement;
const effect = () => screen.getByTestId("save-version-effect");

describe("SaveVersionModal (ADR-0088)", () => {
  beforeEach(() => toast.mockReset());

  it("offers three exclusive outcomes, the new version first, titled vN and selected", () => {
    renderModal();
    expect(screen.getByTestId("save-version-modal")).toHaveTextContent("Save feature-with-review");
    expect(screen.getAllByRole("radio")).toHaveLength(3);
    expect(choice("New version v9")).toHaveAttribute("aria-checked", "true");
    expect(choice("Keep v8 apart")).toHaveAttribute("aria-checked", "false");
    expect(choice("Replace v8")).toHaveAttribute("aria-checked", "false");
    expect(titleInput().value).toBe("v9");
    expect(document.activeElement).toBe(titleInput());
    expect(effect()).toHaveTextContent("v8 stays in Versions");
    expect(confirm()).toHaveTextContent("Save v9");
    expect(screen.queryByRole("checkbox")).toBeNull();
  });

  it("saves a new version under the title typed, then says so", async () => {
    const { onSave } = renderModal();
    fireEvent.change(titleInput(), { target: { value: "Split prompt" } });
    fireEvent.click(confirm());
    expect(onSave).toHaveBeenCalledWith({
      new_version: true,
      title: "Split prompt",
      keep_old: false,
    });
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Saved feature-with-review v9", "success"));
  });

  it("keeps the last version apart as a pipeline of its own, the button names both", async () => {
    const { onSave } = renderModal("external edit", { ...V9, kept: { id: "feature-with-review-v8" } });
    fireEvent.click(choice("Keep v8 apart"));
    expect(effect()).toHaveTextContent("v8 also kept as feature-with-review (v8)");
    expect(confirm()).toHaveTextContent("Save v9, keep v8");
    expect(titleInput()).toBeEnabled();
    fireEvent.click(confirm());
    expect(onSave).toHaveBeenCalledWith({
      new_version: true,
      title: "v9",
      keep_old: true,
    });
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith("Saved feature-with-review v9 · kept feature-with-review (v8)", "success"),
    );
  });

  it("replaces the last version under its number, its title shown and not editable", async () => {
    const { onSave } = renderModal("external edit", { ok: true, version: { number: 8, title: "v8" } });
    fireEvent.click(choice("Replace v8"));
    expect(titleInput()).toBeDisabled();
    expect(titleInput().value).toBe("external edit");
    expect(effect()).toHaveTextContent("Overwrites v8 « external edit », same number");
    expect(confirm()).toHaveTextContent("Replace v8");
    expect(confirm()).toHaveClass("d-btn--danger");
    fireEvent.click(confirm());
    expect(onSave).toHaveBeenCalledWith({
      new_version: false,
      keep_old: false,
    });
    await waitFor(() => expect(toast).toHaveBeenCalledWith("Replaced feature-with-review v8", "success"));
  });

  it("names no title the last version only has by default", () => {
    renderModal("v8");
    fireEvent.click(choice("Replace v8"));
    expect(effect()).toHaveTextContent(/^Overwrites v8, same number$/);
  });

  it("moves between the outcomes with the arrow keys", () => {
    renderModal();
    fireEvent.keyDown(choice("New version v9"), { key: "ArrowRight" });
    expect(choice("Keep v8 apart")).toHaveAttribute("aria-checked", "true");
  });

  it("saves on Enter in the title", () => {
    const { onSave } = renderModal();
    fireEvent.keyDown(titleInput(), { key: "Enter" });
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("saves on Ctrl+Enter, says nothing when the save failed", async () => {
    const { onSave } = renderModal("v8", null);
    fireEvent.keyDown(screen.getByTestId("save-version-modal"), {
      key: "Enter",
      ctrlKey: true,
    });
    await waitFor(() => expect(onSave).toHaveBeenCalledTimes(1));
    await waitFor(() => expect(confirm()).toBeEnabled());
    expect(toast).not.toHaveBeenCalled();
  });

  it("names the version the daemon numbered, when another Save came first", async () => {
    renderModal("external edit", { ok: true, version: { number: 10, title: "v10" }, kept: { id: "x" } });
    fireEvent.click(choice("Keep v8 apart"));
    fireEvent.click(confirm());
    await waitFor(() =>
      expect(toast).toHaveBeenCalledWith("Saved feature-with-review v10 · kept feature-with-review (v9)", "success"),
    );
  });

  it("saves once on Enter pressed twice", async () => {
    const { onSave } = renderModal();
    fireEvent.keyDown(titleInput(), { key: "Enter" });
    fireEvent.keyDown(titleInput(), { key: "Enter" });
    await waitFor(() => expect(toast).toHaveBeenCalledTimes(1));
    expect(onSave).toHaveBeenCalledTimes(1);
  });

  it("closes on Cancel and on Escape, saving nothing", () => {
    const { onClose, onSave } = renderModal();
    fireEvent.click(screen.getByTestId("save-version-cancel"));
    fireEvent.keyDown(screen.getByTestId("save-version-modal"), {
      key: "Escape",
    });
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(onSave).not.toHaveBeenCalled();
  });
});
