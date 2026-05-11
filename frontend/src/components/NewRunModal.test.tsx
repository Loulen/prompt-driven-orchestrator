import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import type { LibraryPipelineEntry } from "../api";
import { useEditStore } from "../stores/editStore";

vi.mock("../api", () => ({
  fetchPipelines: vi.fn().mockResolvedValue([]),
  createRun: vi.fn().mockResolvedValue({ run_id: "test-run" }),
}));

const noop = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    pipelines: [],
  });
});

function renderModal(libraryPipelines: LibraryPipelineEntry[] = []) {
  return render(
    <NewRunModal
      open={true}
      onClose={noop}
      onCreated={noop}
      libraryPipelines={libraryPipelines}
    />,
  );
}

describe("NewRunModal with library pipelines", () => {
  it("shows starred templates in the dropdown when library pipelines exist", () => {
    const pipelines: LibraryPipelineEntry[] = [
      { id: "review", name: "Review Pipeline", node_count: 5, modified: null, yaml: "" },
    ];
    renderModal(pipelines);

    const select = screen.getByRole("combobox") as HTMLSelectElement;
    const optgroup = select.querySelector('optgroup[label="★ Starred templates"]');
    expect(optgroup).not.toBeNull();
    expect(optgroup!.querySelector("option")!.textContent).toBe("Review Pipeline");
  });

  it("shows empty state message when no library pipelines and no pipelines exist", () => {
    renderModal([]);

    const option = screen.getByText(
      /Star a template from the info panel to make it launchable/,
    );
    expect(option).toBeInTheDocument();
  });

  it("pre-selects the first library pipeline when available", () => {
    const pipelines: LibraryPipelineEntry[] = [
      { id: "deploy", name: "Deploy Pipeline", node_count: 3, modified: null, yaml: "" },
    ];
    renderModal(pipelines);

    const select = screen.getByRole("combobox") as HTMLSelectElement;
    expect(select.value).toBe("__lib__deploy");
  });
});
