import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import type { LibraryPipelineEntry } from "../api";
import { useEditStore } from "../stores/editStore";

vi.mock("../api", () => ({
  fetchPipelines: vi.fn().mockResolvedValue([]),
  createRun: vi.fn().mockResolvedValue({ run_id: "test-run" }),
  validateRepo: vi.fn().mockResolvedValue({ valid: true }),
  listBranches: vi.fn().mockResolvedValue(["main", "dev", "feature-x"]),
}));

const { validateRepo, listBranches, createRun } = await import("../api");

const noop = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  vi.useFakeTimers({ shouldAdvanceTime: true });
  useEditStore.setState({
    openTabs: [],
    activeTabId: null,
    pipelines: [],
  });
});

afterEach(() => {
  vi.useRealTimers();
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

async function enterValidRepo(value = "/home/user/project") {
  const repoInput = screen.getByLabelText(/target repository/i);
  fireEvent.change(repoInput, { target: { value } });
  await vi.advanceTimersByTimeAsync(500);
  await waitFor(() => {
    expect(validateRepo).toHaveBeenCalledWith(value);
  });
  await waitFor(() => {
    expect(listBranches).toHaveBeenCalledWith(value);
  });
}

describe("NewRunModal with library pipelines", () => {
  it("shows starred templates in the dropdown when library pipelines exist", async () => {
    const pipelines: LibraryPipelineEntry[] = [
      { id: "review", name: "Review Pipeline", scope: "repo", node_count: 5, modified: null, yaml: "", prompts: {} },
    ];
    renderModal(pipelines);
    await enterValidRepo();

    const select = screen.getByLabelText(/pipeline/i) as HTMLSelectElement;
    const optgroup = select.querySelector('optgroup[label="★ Starred templates"]');
    expect(optgroup).not.toBeNull();
    expect(optgroup!.querySelector("option")!.textContent).toBe("Review Pipeline");
  });

  it("shows empty state message when no library pipelines and no pipelines exist", async () => {
    renderModal([]);
    await enterValidRepo();

    const option = screen.getByText(/no pipelines found/i);
    expect(option).toBeInTheDocument();
  });

  it("pre-selects the first library pipeline when available", async () => {
    const pipelines: LibraryPipelineEntry[] = [
      { id: "deploy", name: "Deploy Pipeline", scope: "repo", node_count: 3, modified: null, yaml: "", prompts: {} },
    ];
    renderModal(pipelines);
    await enterValidRepo();

    const select = screen.getByLabelText(/pipeline/i) as HTMLSelectElement;
    expect(select.value).toBe("__lib__deploy");
  });
});

describe("NewRunModal — multi-repo form flow", () => {
  it("renders a target repo input field", () => {
    renderModal();
    expect(screen.getByLabelText(/target repository/i)).toBeInTheDocument();
  });

  it("validates the repo path and shows error for invalid repo", async () => {
    vi.mocked(validateRepo).mockResolvedValueOnce({ valid: false, error: "not a git repository" });

    renderModal();
    const repoInput = screen.getByLabelText(/target repository/i);
    fireEvent.change(repoInput, { target: { value: "/tmp/not-a-repo" } });
    await vi.advanceTimersByTimeAsync(500);

    await waitFor(() => {
      expect(validateRepo).toHaveBeenCalledWith("/tmp/not-a-repo");
    });
    await waitFor(() => {
      expect(screen.getByText(/not a git repository/i)).toBeInTheDocument();
    });
  });

  it("fetches branches after valid repo is entered", async () => {
    renderModal();
    const repoInput = screen.getByLabelText(/target repository/i);
    fireEvent.change(repoInput, { target: { value: "/home/user/project" } });
    await vi.advanceTimersByTimeAsync(500);

    await waitFor(() => {
      expect(validateRepo).toHaveBeenCalledWith("/home/user/project");
    });
    await waitFor(() => {
      expect(listBranches).toHaveBeenCalledWith("/home/user/project");
    });
  });

  it("renders a source branch dropdown populated after repo validation", async () => {
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      const branchSelect = screen.getByLabelText(/source branch/i) as HTMLSelectElement;
      const options = Array.from(branchSelect.options).map((o) => o.value);
      expect(options).toContain("main");
      expect(options).toContain("dev");
      expect(options).toContain("feature-x");
    });
  });

  it("passes target_repo and source_branch to createRun on launch", async () => {
    const onCreated = vi.fn();
    const pipelines: LibraryPipelineEntry[] = [
      { id: "p1", name: "Test Pipeline", scope: "repo", node_count: 2, modified: null, yaml: "", prompts: {} },
    ];

    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={onCreated}
        libraryPipelines={pipelines}
      />,
    );

    await enterValidRepo();

    const branchSelect = screen.getByLabelText(/source branch/i) as HTMLSelectElement;
    fireEvent.change(branchSelect, { target: { value: "dev" } });

    const inputTextarea = screen.getByPlaceholderText(/free-text prompt/i);
    fireEvent.change(inputTextarea, { target: { value: "implement feature X" } });

    vi.useRealTimers();
    const launchButton = screen.getByRole("button", { name: /launch/i });
    fireEvent.click(launchButton);

    await waitFor(() => {
      expect(createRun).toHaveBeenCalledWith(
        expect.objectContaining({
          target_repo: "/home/user/project",
          source_branch: "dev",
          input: "implement feature X",
        }),
      );
    });
  });

  it("does not show branch dropdown before repo is validated", () => {
    renderModal();
    expect(screen.queryByLabelText(/source branch/i)).not.toBeInTheDocument();
  });

  it("clears branches when repo path changes", async () => {
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      expect(screen.getByLabelText(/source branch/i)).toBeInTheDocument();
    });

    vi.mocked(validateRepo).mockResolvedValueOnce({ valid: false, error: "not a git repository" });
    const repoInput = screen.getByLabelText(/target repository/i);
    fireEvent.change(repoInput, { target: { value: "/home/user/other" } });
    await vi.advanceTimersByTimeAsync(500);

    await waitFor(() => {
      expect(screen.queryByLabelText(/source branch/i)).not.toBeInTheDocument();
    });
  });
});
