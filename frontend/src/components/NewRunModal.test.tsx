import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import { useEditStore } from "../stores/editStore";
import type { PipelineListEntry } from "../types";

const makePipeline = (overrides: Partial<PipelineListEntry> = {}): PipelineListEntry => ({
  id: "test-pipe",
  name: "Test Pipeline",
  scope: "repo",
  path: "/repo/.maestro/pipelines/test-pipe.yaml",
  node_count: 3,
  modified: null,
  variables: {},
  ...overrides,
});

vi.mock("../api", () => ({
  fetchPipelines: vi.fn().mockResolvedValue([]),
  createRun: vi.fn().mockResolvedValue({ run_id: "test-run" }),
  validateRepo: vi.fn().mockResolvedValue({ valid: true }),
  listBranches: vi.fn().mockResolvedValue(["main", "dev", "feature-x"]),
  promotePipeline: vi.fn().mockResolvedValue({ id: "test-pipe", drifted: false }),
}));

const { validateRepo, listBranches, createRun, fetchPipelines, promotePipeline } = await import("../api");

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

function renderModal() {
  return render(
    <NewRunModal
      open={true}
      onClose={noop}
      onCreated={noop}
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

describe("NewRunModal — grouped pipeline picker", () => {
  it("shows repo pipelines in the Repo group", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "review", name: "Review Pipeline", scope: "repo" }),
    ]);
    renderModal();
    await enterValidRepo();

    const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
    const optgroup = select.querySelector('optgroup[label="Repo pipelines"]');
    expect(optgroup).not.toBeNull();
    expect(optgroup!.querySelector("option")!.textContent).toBe("Review Pipeline");
  });

  it("shows library pipelines in the Library group", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "lib-pipe", name: "Library Pipeline", scope: "library" }),
    ]);
    renderModal();
    await enterValidRepo();

    const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
    const optgroup = select.querySelector('optgroup[label="★ Library"]');
    expect(optgroup).not.toBeNull();
    expect(optgroup!.querySelector("option")!.textContent).toBe("Library Pipeline");
  });

  it("shows repo pipelines before library pipelines", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "lib-pipe", name: "Library Pipeline", scope: "library" }),
      makePipeline({ id: "repo-pipe", name: "Repo Pipeline", scope: "repo" }),
    ]);
    renderModal();
    await enterValidRepo();

    const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
    const groups = Array.from(select.querySelectorAll("optgroup"));
    expect(groups.length).toBeGreaterThanOrEqual(2);
    expect(groups[0].label).toBe("Repo pipelines");
    expect(groups[1].label).toBe("★ Library");
  });

  it("shows empty state when no pipelines found", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([]);
    renderModal();
    await enterValidRepo();

    const option = screen.getByText(/no pipelines found/i);
    expect(option).toBeInTheDocument();
  });

  it("pre-selects the first repo pipeline when available", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "first-repo", name: "First Repo", scope: "repo" }),
      makePipeline({ id: "lib-pipe", name: "Lib", scope: "library" }),
    ]);
    renderModal();
    await enterValidRepo();

    const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
    expect(select.value).toBe("first-repo");
  });
});

describe("NewRunModal — drift indicator", () => {
  it("shows drift warning text for drifted library pipeline", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "drifted", name: "Drifted Pipe", scope: "library", drifted: true }),
    ]);
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      expect(screen.getByTestId("drift-indicator")).toBeInTheDocument();
    });
    expect(screen.getByTestId("drift-warning")).toBeInTheDocument();
  });

  it("shows filled star without dot for synced library pipeline", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "synced", name: "Synced Pipe", scope: "library", drifted: false }),
    ]);
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      expect(screen.getByTestId("library-star")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("drift-indicator")).not.toBeInTheDocument();
  });

  it("prefixes drifted library pipeline name with warning icon in dropdown", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "drifted", name: "Drifted Pipe", scope: "library", drifted: true }),
    ]);
    renderModal();
    await enterValidRepo();

    const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
    const option = select.querySelector('optgroup[label="★ Library"] option');
    expect(option!.textContent).toContain("⚠");
  });
});

describe("NewRunModal — promote button", () => {
  it("shows promote button for selected repo pipeline", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "repo-pipe", name: "Repo Pipeline", scope: "repo" }),
    ]);
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      expect(screen.getByTestId("promote-button")).toBeInTheDocument();
    });
  });

  it("calls promotePipeline when promote button is clicked", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "repo-pipe", name: "Repo Pipeline", scope: "repo" }),
    ]);
    renderModal();
    await enterValidRepo();

    vi.useRealTimers();
    const button = screen.getByTestId("promote-button");
    fireEvent.click(button);

    await waitFor(() => {
      expect(promotePipeline).toHaveBeenCalledWith("repo-pipe");
    });
  });

  it("does not show promote button for library pipelines", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "lib-pipe", name: "Lib Pipe", scope: "library" }),
    ]);
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      expect(screen.getByTestId("library-star")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("promote-button")).not.toBeInTheDocument();
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
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Test Pipeline", scope: "repo" }),
    ]);

    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={onCreated}
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

  it("keeps Launch disabled with an empty prompt for a prompt-required pipeline", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Required Pipeline", scope: "repo", prompt_required: true }),
    ]);
    renderModal();
    await enterValidRepo();

    const launchButton = screen.getByRole("button", { name: /launch/i });
    expect(launchButton).toBeDisabled();
  });

  it("enables Launch with an empty prompt for a prompt-optional pipeline", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Optional Pipeline", scope: "repo", prompt_required: false }),
    ]);
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      const launchButton = screen.getByRole("button", { name: /launch/i });
      expect(launchButton).toBeEnabled();
    });
  });

  it("launches a prompt-optional pipeline with empty input", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Optional Pipeline", scope: "repo", prompt_required: false }),
    ]);
    renderModal();
    await enterValidRepo();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: /launch/i })).toBeEnabled();
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByRole("button", { name: /launch/i }));

    await waitFor(() => {
      expect(createRun).toHaveBeenCalledWith(
        expect.objectContaining({ pipeline: "Optional Pipeline", input: "" }),
      );
    });
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

describe("NewRunModal — image upload", () => {
  it("renders the image upload area", async () => {
    renderModal();
    expect(screen.getByTestId("image-drop-zone")).toBeInTheDocument();
    expect(screen.getByTestId("image-upload-button")).toBeInTheDocument();
    expect(screen.getByText(/paste, drag-drop, or click/i)).toBeInTheDocument();
  });

  it("shows 'Optional' hint when no images attached", () => {
    renderModal();
    expect(screen.getByText(/optional/i)).toBeInTheDocument();
  });

  it("adds images via file input and shows thumbnails", async () => {
    renderModal();
    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;

    const file = new File(["png-data"], "screenshot.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      const thumbnails = screen.getAllByTestId("image-thumbnail");
      expect(thumbnails).toHaveLength(1);
    });
    expect(screen.getByText("1 image attached")).toBeInTheDocument();
  });

  it("shows remove button and removes image on click", async () => {
    renderModal();
    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;

    const file = new File(["png-data"], "test.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(1);
    });

    const removeBtn = screen.getByTestId("image-remove-button");
    fireEvent.click(removeBtn);

    await waitFor(() => {
      expect(screen.queryAllByTestId("image-thumbnail")).toHaveLength(0);
    });
    expect(screen.getByText(/optional/i)).toBeInTheDocument();
  });

  it("supports multiple images", async () => {
    renderModal();
    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;

    const file1 = new File(["a"], "one.png", { type: "image/png" });
    const file2 = new File(["b"], "two.jpg", { type: "image/jpeg" });
    fireEvent.change(fileInput, { target: { files: [file1, file2] } });

    await waitFor(() => {
      expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(2);
    });
    expect(screen.getByText("2 images attached")).toBeInTheDocument();
  });

  it("shows add-more button when images exist", async () => {
    renderModal();
    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;

    const file = new File(["png"], "img.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getByTestId("image-add-more-button")).toBeInTheDocument();
    });
  });

  it("passes images to createRun on launch", async () => {
    const onCreated = vi.fn();
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Test Pipeline" }),
    ]);

    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={onCreated}
      />,
    );

    await enterValidRepo();

    const inputTextarea = screen.getByPlaceholderText(/free-text prompt/i);
    fireEvent.change(inputTextarea, { target: { value: "implement feature" } });

    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;
    const file = new File(["png-data"], "design.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(1);
    });

    vi.useRealTimers();
    const launchButton = screen.getByRole("button", { name: /launch/i });
    fireEvent.click(launchButton);

    await waitFor(() => {
      expect(createRun).toHaveBeenCalledWith(
        expect.objectContaining({
          input: "implement feature",
          images: expect.arrayContaining([
            expect.objectContaining({ name: "design.png" }),
          ]),
        }),
      );
    });
  });

  it("does not pass images when none attached", async () => {
    const onCreated = vi.fn();
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Test Pipeline" }),
    ]);

    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={onCreated}
      />,
    );

    await enterValidRepo();

    const inputTextarea = screen.getByPlaceholderText(/free-text prompt/i);
    fireEvent.change(inputTextarea, { target: { value: "text only" } });

    vi.useRealTimers();
    const launchButton = screen.getByRole("button", { name: /launch/i });
    fireEvent.click(launchButton);

    await waitFor(() => {
      expect(createRun).toHaveBeenCalledWith(
        expect.objectContaining({
          input: "text only",
          images: undefined,
        }),
      );
    });
  });

  it("filters non-image files from file input", async () => {
    renderModal();
    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;

    const textFile = new File(["text"], "notes.txt", { type: "text/plain" });
    const imageFile = new File(["png"], "img.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [textFile, imageFile] } });

    await waitFor(() => {
      expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(1);
    });
  });
});

describe("NewRunModal — form persistence across close/reopen", () => {
  it("preserves prompt text across close/reopen", async () => {
    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop} />,
    );

    await enterValidRepo();

    const textarea = screen.getByPlaceholderText(/free-text prompt/i);
    fireEvent.change(textarea, { target: { value: "my prompt text" } });

    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop} />);
    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop} />);

    expect(screen.getByPlaceholderText(/free-text prompt/i)).toHaveValue("my prompt text");
  });

  it("preserves target repo and pipeline selection across close/reopen", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "My Pipeline", scope: "repo" }),
    ]);

    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop} />,
    );

    await enterValidRepo("/home/user/my-repo");

    await waitFor(() => {
      const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
      expect(select.value).toBe("p1");
    });

    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop} />);
    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop} />);

    const repoInput = screen.getByLabelText(/target repository/i) as HTMLInputElement;
    expect(repoInput.value).toBe("/home/user/my-repo");

    const select = screen.getByTestId("pipeline-select") as HTMLSelectElement;
    expect(select.value).toBe("p1");
  });

  it("preserves images across close/reopen", async () => {
    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop} />,
    );

    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;
    const file = new File(["png-data"], "screenshot.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(1);
    });

    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop} />);
    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop} />);

    expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(1);
  });

  it("resets form fields after successful launch", async () => {
    const onCreated = vi.fn();
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Test Pipeline", scope: "repo" }),
    ]);

    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={onCreated} />,
    );

    await enterValidRepo();

    const textarea = screen.getByPlaceholderText(/free-text prompt/i);
    fireEvent.change(textarea, { target: { value: "implement feature" } });

    const fileInput = screen.getByTestId("image-file-input") as HTMLInputElement;
    const file = new File(["png-data"], "design.png", { type: "image/png" });
    fireEvent.change(fileInput, { target: { files: [file] } });

    await waitFor(() => {
      expect(screen.getAllByTestId("image-thumbnail")).toHaveLength(1);
    });

    vi.useRealTimers();
    const launchButton = screen.getByRole("button", { name: /launch/i });
    fireEvent.click(launchButton);

    await waitFor(() => {
      expect(onCreated).toHaveBeenCalledWith("test-run");
    });

    rerender(<NewRunModal open={true} onClose={noop} onCreated={onCreated} />);

    expect(screen.getByPlaceholderText(/free-text prompt/i)).toHaveValue("");
    expect(screen.queryAllByTestId("image-thumbnail")).toHaveLength(0);
  });
});

describe("NewRunModal run name field", () => {
  it("renders a name input and auto-generated checkbox", () => {
    renderModal();

    expect(screen.getByTestId("run-name-input")).toBeInTheDocument();
    expect(screen.getByTestId("auto-name-checkbox")).toBeInTheDocument();
    expect(screen.getByText("Auto-generated by manager")).toBeInTheDocument();
  });

  it("name input is disabled when auto-generated is checked", () => {
    renderModal();

    const input = screen.getByTestId("run-name-input") as HTMLInputElement;
    const checkbox = screen.getByTestId("auto-name-checkbox") as HTMLInputElement;

    expect(checkbox.checked).toBe(true);
    expect(input.disabled).toBe(true);
  });

  it("name field is the first field in the modal body", () => {
    renderModal();

    const labels = screen.getAllByText(/^(Name|Pipeline|Input)$/);
    expect(labels[0].textContent).toBe("Name");
  });
});
