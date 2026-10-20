import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import SaveVersionModal from "./SaveVersionModal";

function renderModal(title = "external edit") {
  const onSave = vi.fn().mockResolvedValue(undefined);
  const onClose = vi.fn();
  render(
    <SaveVersionModal pipelineName="feature-with-review" last={{ number: 8, title }} onSave={onSave} onClose={onClose} />,
  );
  return { onSave, onClose };
}

describe("SaveVersionModal (ADR-0088)", () => {
  it("offers the next version, titled vN and selected, the old one kept in the history", () => {
    renderModal();
    expect(screen.getByTestId("save-version-modal")).toHaveTextContent("Save feature-with-review");
    expect(screen.getByTestId("save-version-new")).toBeChecked();
    const title = screen.getByTestId("save-version-title-input") as HTMLInputElement;
    expect(title.value).toBe("v9");
    expect(document.activeElement).toBe(title);
    expect(screen.getByTestId("save-version-keep-hint")).toHaveTextContent("v8 is only kept in the history.");
    expect(screen.getByTestId("save-version-confirm")).toHaveTextContent("Save v9");
  });

  it("saves a new version under the title typed", () => {
    const { onSave } = renderModal();
    fireEvent.change(screen.getByTestId("save-version-title-input"), { target: { value: "Split prompt" } });
    fireEvent.click(screen.getByTestId("save-version-confirm"));
    expect(onSave).toHaveBeenCalledWith({ new_version: true, title: "Split prompt", keep_old: false });
  });

  it("unticked, replaces the last version under its number and says so", () => {
    const { onSave } = renderModal();
    fireEvent.click(screen.getByTestId("save-version-new"));
    expect(screen.queryByTestId("save-version-title-input")).not.toBeInTheDocument();
    expect(screen.getByTestId("save-version-replaces")).toHaveTextContent(
      "Replaces v8 « external edit » and keeps its number. Runs launched on v8 keep showing v8.",
    );
    fireEvent.click(screen.getByTestId("save-version-keep-old"));
    expect(screen.getByTestId("save-version-keep-hint")).toHaveTextContent(
      "v8 stays as a separate pipeline feature-with-review (v8) in the Library.",
    );
    expect(screen.getByTestId("save-version-confirm")).toHaveTextContent("Replace v8");
    fireEvent.click(screen.getByTestId("save-version-confirm"));
    expect(onSave).toHaveBeenCalledWith({ new_version: false, keep_old: true });
  });

  it("names no title the last version only has by default", () => {
    renderModal("v8");
    fireEvent.click(screen.getByTestId("save-version-new"));
    expect(screen.getByTestId("save-version-replaces")).toHaveTextContent(/^Replaces v8 and keeps its number/);
  });

  it("closes on Cancel and on Escape", () => {
    const { onClose } = renderModal();
    fireEvent.click(screen.getByTestId("save-version-cancel"));
    fireEvent.keyDown(document, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(2);
  });
});
