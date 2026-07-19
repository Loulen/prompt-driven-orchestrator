import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import { useEditStore } from "../stores/editStore";
import type { PipelineListEntry } from "../types";

const makePipeline = (overrides: Partial<PipelineListEntry> = {}): PipelineListEntry => ({
  id: "test-pipe",
  name: "Test Pipeline",
  scope: "repo",
  path: "/repo/.pdo/pipelines/test-pipe.yaml",
  node_count: 3,
  modified: null,
  variables: {},
  ...overrides,
});

vi.mock("../api", () => ({
  fetchPipelines: vi.fn().mockResolvedValue([]),
  createRun: vi.fn().mockResolvedValue({ run_id: "test-run" }),
  createTrigger: vi.fn().mockResolvedValue({ id: "trg-test" }),
  updateTrigger: vi.fn().mockResolvedValue({ id: "trg-test" }),
  validateRepo: vi.fn().mockResolvedValue({ valid: true }),
  listBranches: vi.fn().mockResolvedValue(["main", "dev", "feature-x"]),
  promotePipeline: vi.fn().mockResolvedValue({ id: "test-pipe", drifted: false }),
  testGuard: vi.fn().mockResolvedValue({
    outcome: "pass",
    stdout: "",
    stderr: "",
    exit_code: 0,
    detail: null,
  }),
}));

const { validateRepo, listBranches, createRun, createTrigger, updateTrigger, fetchPipelines, promotePipeline, testGuard } = await import("../api");

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

describe("NewRunModal — Trigger mode (#160)", () => {
  async function selectPipelineAndRepo() {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Auditor", scope: "repo", prompt_required: false }),
    ]);
    renderModal();
    await enterValidRepo();
  }

  it("offers a [Run now | Trigger] toggle and defaults to Run now", () => {
    renderModal();
    expect(screen.getByTestId("mode-run")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "false");
  });

  it("switches the footer action to Create trigger in Trigger mode", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    expect(screen.getByTestId("create-trigger-button")).toBeInTheDocument();
    expect(screen.queryByTestId("launch-button")).not.toBeInTheDocument();
  });

  it("exposes schedule presets and a raw cron escape hatch", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    expect(screen.getByTestId("preset-every_15_min")).toBeInTheDocument();
    expect(screen.getByTestId("preset-hourly")).toBeInTheDocument();
    expect(screen.getByTestId("preset-daily")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("preset-custom"));
    expect(screen.getByTestId("raw-cron-input")).toBeInTheDocument();
  });

  it("relabels the prompt field as an optional input template in Trigger mode", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    expect(screen.getByText(/input template \(optional\)/i)).toBeInTheDocument();
  });

  it("creates a trigger with the compiled cron and chosen pipeline", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Nightly audit" },
    });
    fireEvent.click(screen.getByTestId("preset-every_15_min"));

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("create-trigger-button"));

    await waitFor(() => {
      expect(createTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Nightly audit",
          pipeline_id: "p1",
          cron: "*/15 * * * *",
          target_repo: "/home/user/project",
        }),
      );
    });
  });

  it("keeps Create trigger disabled until a name is entered", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    expect(screen.getByTestId("create-trigger-button")).toBeDisabled();
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Audit" },
    });
    expect(screen.getByTestId("create-trigger-button")).not.toBeDisabled();
  });

  it("surfaces the server reject reason inline", async () => {
    await selectPipelineAndRepo();
    vi.mocked(createTrigger).mockRejectedValueOnce(
      new Error("this pipeline requires a prompt; add a guard, an input template, ..."),
    );
    fireEvent.click(screen.getByTestId("mode-trigger"));
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Bad" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("create-trigger-button"));

    await waitFor(() => {
      expect(screen.getByText(/requires a prompt/i)).toBeInTheDocument();
    });
  });

  it("disables Create and shows the reason when a prompt-required pipeline has no guard or input (#161)", async () => {
    // A pipeline that requires a prompt; with no guard and no input template,
    // the modal must disable Create and explain why, mirroring the server reject.
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Needs Prompt", scope: "repo", prompt_required: true }),
    ]);
    renderModal();
    await enterValidRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Bad" },
    });

    expect(screen.getByTestId("create-trigger-button")).toBeDisabled();
    expect(screen.getByText(/requires a prompt/i)).toBeInTheDocument();

    // Adding a guard command resolves the misconfiguration.
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "gh issue list" },
    });
    expect(screen.getByTestId("create-trigger-button")).not.toBeDisabled();
  });

  it("exposes a guard command field with its contract helper text (#161)", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    expect(screen.getByTestId("guard-command-input")).toBeInTheDocument();
    // The contract is explained inline so a correct guard can be written
    // without reading docs (exit 0 fires, non-zero skips, stdout = input).
    expect(screen.getByText(/exit 0 fires/i)).toBeInTheDocument();
    expect(screen.getByText(/stdout/i)).toBeInTheDocument();
  });

  it("passes the guard command to createTrigger (#161)", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Fixer" },
    });
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "gh issue list --label ready-for-agent" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("create-trigger-button"));

    await waitFor(() => {
      expect(createTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Fixer",
          guard_command: "gh issue list --label ready-for-agent",
        }),
      );
    });
  });

  // --- #239: bounded-allow overlap control ---

  it("reveals the max-concurrent input only when the allow checkbox is checked", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    expect(screen.getByTestId("overlap-allow-checkbox")).toBeInTheDocument();
    expect(screen.queryByTestId("max-concurrent-input")).not.toBeInTheDocument();

    fireEvent.click(screen.getByTestId("overlap-allow-checkbox"));
    expect(screen.getByTestId("max-concurrent-input")).toBeInTheDocument();
  });

  it("creates an allow trigger with the chosen max_concurrent cap", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Bounded" },
    });
    fireEvent.click(screen.getByTestId("overlap-allow-checkbox"));
    fireEvent.change(screen.getByTestId("max-concurrent-input"), {
      target: { value: "2" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("create-trigger-button"));

    await waitFor(() => {
      expect(createTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Bounded",
          overlap_policy: "allow",
          max_concurrent: 2,
        }),
      );
    });
  });

  it("creates a skip trigger with no cap when the box is unchecked", async () => {
    await selectPipelineAndRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    fireEvent.change(screen.getByTestId("trigger-name-input"), {
      target: { value: "Default" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("create-trigger-button"));

    await waitFor(() => {
      expect(createTrigger).toHaveBeenCalledWith(
        expect.objectContaining({
          name: "Default",
          overlap_policy: "skip",
          max_concurrent: undefined,
        }),
      );
    });
  });
});

describe("NewRunModal — Test guard dry-run (#350)", () => {
  async function enterTriggerMode(promptRequired = false) {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Auditor", scope: "repo", prompt_required: promptRequired }),
    ]);
    renderModal();
    await enterValidRepo();
    fireEvent.click(screen.getByTestId("mode-trigger"));
  }

  it("disables Test guard (with a tooltip) until the target repo is valid", () => {
    renderModal();
    fireEvent.click(screen.getByTestId("mode-trigger"));
    const button = screen.getByTestId("guard-test-button");
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("title", "Select a valid target repository first");
  });

  it("keeps Test guard disabled with a valid repo but an empty guard command, then enables it", async () => {
    await enterTriggerMode();
    expect(screen.getByTestId("guard-test-button")).toBeDisabled();
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "gh issue list" },
    });
    expect(screen.getByTestId("guard-test-button")).not.toBeDisabled();
  });

  it("shows 'Would fire' and the stdout for a passing guard", async () => {
    vi.mocked(testGuard).mockResolvedValueOnce({
      outcome: "pass",
      stdout: "issue-123\n",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    await enterTriggerMode();
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "gh issue list" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-verdict")).toHaveTextContent("Would fire");
    expect(screen.getByTestId("guard-test-output")).toHaveTextContent("issue-123");
    expect(testGuard).toHaveBeenCalledWith("gh issue list", "/home/user/project");
  });

  it("shows 'Would skip' with the exit code and stderr for a non-zero guard", async () => {
    vi.mocked(testGuard).mockResolvedValueOnce({
      outcome: "skip",
      stdout: "",
      stderr: "no work to do",
      exit_code: 3,
      detail: null,
    });
    await enterTriggerMode();
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "exit 3" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-verdict")).toHaveTextContent("Would skip");
    const output = screen.getByTestId("guard-test-output");
    expect(output).toHaveTextContent("3");
    expect(output).toHaveTextContent("no work to do");
  });

  it("shows 'Guard error' and surfaces a request failure inline", async () => {
    vi.mocked(testGuard).mockRejectedValueOnce(new Error("boom"));
    await enterTriggerMode();
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "sleep 99" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("guard-test-button"));

    expect(await screen.findByTestId("guard-test-error")).toHaveTextContent("boom");
    expect(screen.queryByTestId("guard-test-result")).not.toBeInTheDocument();
  });

  it("clears a stale verdict when the guard command is edited", async () => {
    vi.mocked(testGuard).mockResolvedValueOnce({
      outcome: "pass",
      stdout: "ok",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    await enterTriggerMode();
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "true" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("guard-test-button"));
    await screen.findByTestId("guard-test-result");

    // Editing the command invalidates the verdict.
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "true # edited" },
    });
    expect(screen.queryByTestId("guard-test-result")).not.toBeInTheDocument();
  });

  it("shows the would-reject caveat only for a prompt-required pipeline whose input would be empty", async () => {
    vi.mocked(testGuard).mockResolvedValue({
      outcome: "pass",
      stdout: "",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    // Prompt-required pipeline, empty input, guard passes with empty stdout.
    await enterTriggerMode(true);
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "true" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("guard-test-button"));

    await screen.findByTestId("guard-test-result");
    expect(screen.getByTestId("guard-test-caveat")).toBeInTheDocument();
  });

  it("omits the caveat for a prompt-optional pipeline even with empty stdout", async () => {
    vi.mocked(testGuard).mockResolvedValue({
      outcome: "pass",
      stdout: "",
      stderr: "",
      exit_code: 0,
      detail: null,
    });
    await enterTriggerMode(false);
    fireEvent.change(screen.getByTestId("guard-command-input"), {
      target: { value: "true" },
    });

    vi.useRealTimers();
    fireEvent.click(screen.getByTestId("guard-test-button"));

    await screen.findByTestId("guard-test-result");
    expect(screen.queryByTestId("guard-test-caveat")).not.toBeInTheDocument();
  });
});

describe("NewRunModal — run-now and edit from a Trigger (#162)", () => {
  const sampleTrigger = {
    id: "trg-9",
    name: "Nightly audit",
    pipeline_id: "p1",
    pipeline_name: "Auditor",
    target_repo: "/home/user/project",
    source_branch: "dev",
    input_template: "audit the codebase",
    variables: {},
    cron: "*/15 * * * *",
    guard_command: null,
    overlap_policy: "allow",
    enabled: true,
    next_fire_at: null,
    last_fired_at: null,
    last_outcome: null,
  };

  beforeEach(() => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Auditor", scope: "repo", prompt_required: false }),
      makePipeline({ id: "p2", name: "Bugfixer", scope: "repo", prompt_required: false }),
    ]);
  });

  it("edit opens in Trigger mode pre-filled and PATCHes the trigger on submit", async () => {
    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger: sampleTrigger }}
      />,
    );

    // Trigger mode with the existing config prefilled.
    await waitFor(() => {
      expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "true");
    });
    await waitFor(() => {
      expect(screen.getByTestId("trigger-name-input")).toHaveValue("Nightly audit");
    });
    // The footer becomes a Save action (not "Create trigger").
    expect(screen.getByTestId("save-trigger-button")).toBeInTheDocument();
    expect(screen.queryByTestId("create-trigger-button")).not.toBeInTheDocument();

    // Let the debounced repo validation resolve so the form is submittable.
    await vi.advanceTimersByTimeAsync(500);
    await waitFor(() => expect(validateRepo).toHaveBeenCalledWith("/home/user/project"));

    // Edit the schedule and the input template.
    fireEvent.click(screen.getByTestId("preset-hourly"));
    fireEvent.change(screen.getByTestId("input-textarea"), {
      target: { value: "audit harder" },
    });

    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("save-trigger-button")).toBeEnabled());
    fireEvent.click(screen.getByTestId("save-trigger-button"));

    await waitFor(() => {
      expect(updateTrigger).toHaveBeenCalledWith(
        "trg-9",
        expect.objectContaining({
          name: "Nightly audit",
          // The current pipeline is always sent so an unchanged edit is a no-op
          // repoint server-side (#230).
          pipeline_id: "p1",
          cron: "0 * * * *",
          input_template: "audit harder",
        }),
      );
    });
    // Editing never creates a brand-new trigger.
    expect(createTrigger).not.toHaveBeenCalled();
  });

  it("edit-prefills the allow checkbox and cap from a bounded-allow trigger (#239)", async () => {
    const bounded = { ...sampleTrigger, overlap_policy: "allow", max_concurrent: 3 };
    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger: bounded }}
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "true");
    });
    // The box is pre-checked and the cap input pre-filled.
    await waitFor(() => {
      expect(screen.getByTestId("overlap-allow-checkbox")).toBeChecked();
    });
    expect(screen.getByTestId("max-concurrent-input")).toHaveValue(3);

    // Saving round-trips the policy + cap (no silent reset to skip).
    await vi.advanceTimersByTimeAsync(500);
    await waitFor(() => expect(validateRepo).toHaveBeenCalledWith("/home/user/project"));
    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("save-trigger-button")).toBeEnabled());
    fireEvent.click(screen.getByTestId("save-trigger-button"));

    await waitFor(() => {
      expect(updateTrigger).toHaveBeenCalledWith(
        "trg-9",
        expect.objectContaining({
          overlap_policy: "allow",
          max_concurrent: 3,
        }),
      );
    });
  });

  it("edit repoints the trigger to the newly selected pipeline (#230)", async () => {
    render(
      <NewRunModal
        open={true}
        onClose={noop}
        onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger: sampleTrigger }}
      />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "true");
    });
    // Prefilled with the trigger's current pipeline.
    await waitFor(() => {
      expect(screen.getByTestId("pipeline-select")).toHaveValue("p1");
    });

    // Let the debounced repo validation resolve so the form is submittable.
    await vi.advanceTimersByTimeAsync(500);
    await waitFor(() => expect(validateRepo).toHaveBeenCalledWith("/home/user/project"));

    // Change the pipeline — the dropdown is interactive in edit mode, and the
    // change must now actually reach the server (it used to be silently dropped).
    fireEvent.change(screen.getByTestId("pipeline-select"), {
      target: { value: "p2" },
    });

    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("save-trigger-button")).toBeEnabled());
    fireEvent.click(screen.getByTestId("save-trigger-button"));

    await waitFor(() => {
      expect(updateTrigger).toHaveBeenCalledWith(
        "trg-9",
        expect.objectContaining({ pipeline_id: "p2" }),
      );
    });
    expect(createTrigger).not.toHaveBeenCalled();
  });
});

describe("NewRunModal — open-intent reset (#386)", () => {
  const trigger = {
    id: "trg-9",
    name: "Nightly audit",
    pipeline_id: "p1",
    pipeline_name: "Auditor",
    target_repo: "/home/user/project",
    source_branch: "dev",
    input_template: "audit the codebase",
    variables: {},
    cron: "*/15 * * * *",
    guard_command: null,
    overlap_policy: "allow",
    enabled: true,
    next_fire_at: null,
    last_fired_at: null,
    last_outcome: null,
  };

  it("edit → close → open(run) reopens as a clean New Run (kills the stale Edit-Trigger)", async () => {
    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger }} />,
    );

    // Opened in Edit-Trigger mode (footer is Save).
    await waitFor(() => {
      expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "true");
    });
    expect(screen.getByTestId("save-trigger-button")).toBeInTheDocument();

    // Dismiss, then reopen as a plain run.
    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop}
      openIntent={{ kind: "edit-trigger", trigger }} />);
    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop}
      openIntent={{ kind: "run" }} />);

    await waitFor(() => {
      expect(screen.getByTestId("mode-run")).toHaveAttribute("aria-selected", "true");
    });
    expect(screen.getByText("New Run")).toBeInTheDocument();
    expect(screen.getByTestId("launch-button")).toBeInTheDocument();
    // No trigger footer leaks — Finding B (silent PATCH of the wrong trigger).
    expect(screen.queryByTestId("save-trigger-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("create-trigger-button")).not.toBeInTheDocument();
    // Trigger-only fields aren't even rendered in run mode.
    expect(screen.queryByTestId("trigger-name-input")).not.toBeInTheDocument();
  });

  it("edit → close → open(new-trigger) reopens as a blank Create Trigger (Finding B)", async () => {
    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger }} />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("trigger-name-input")).toHaveValue("Nightly audit");
    });
    expect(screen.getByTestId("save-trigger-button")).toBeInTheDocument();

    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop}
      openIntent={{ kind: "edit-trigger", trigger }} />);
    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop}
      openIntent={{ kind: "new-trigger" }} />);

    await waitFor(() => {
      expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "true");
    });
    expect(screen.getByText("New Trigger")).toBeInTheDocument();
    // Footer is Create (a fresh POST), NOT Save: editingTriggerId is cleared, so
    // submitting can't silently PATCH the previously edited trigger.
    expect(screen.getByTestId("create-trigger-button")).toBeInTheDocument();
    expect(screen.queryByTestId("save-trigger-button")).not.toBeInTheDocument();
    // The trigger name is blank, not the edited trigger's name.
    expect(screen.getByTestId("trigger-name-input")).toHaveValue("");
  });

  it("new-trigger opens fresh in Trigger mode with no prior edit", async () => {
    render(
      <NewRunModal open={true} onClose={noop} onCreated={noop}
        openIntent={{ kind: "new-trigger" }} />,
    );

    await waitFor(() => {
      expect(screen.getByTestId("mode-trigger")).toHaveAttribute("aria-selected", "true");
    });
    expect(screen.getByText("New Trigger")).toBeInTheDocument();
    expect(screen.getByTestId("create-trigger-button")).toBeInTheDocument();
    expect(screen.queryByTestId("save-trigger-button")).not.toBeInTheDocument();
  });

  it("edit → close → open(run) clears the shared draft carried from the trigger (#386 Part 2)", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Auditor", scope: "repo", prompt_required: false }),
      makePipeline({ id: "p2", name: "Bugfixer", scope: "repo", prompt_required: false }),
    ]);

    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger }} />,
    );

    // The edit prefilled the shared draft from the trigger (prompt, repo, pipeline).
    await waitFor(() => {
      expect(screen.getByPlaceholderText(/free-text prompt/i)).toHaveValue("audit the codebase");
    });
    await vi.advanceTimersByTimeAsync(500);
    await waitFor(() => expect(validateRepo).toHaveBeenCalledWith("/home/user/project"));
    await waitFor(() => {
      expect(screen.getByTestId("pipeline-select")).toHaveValue("p1");
    });

    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop}
      openIntent={{ kind: "edit-trigger", trigger }} />);
    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop}
      openIntent={{ kind: "run" }} />);

    // A fresh run must not inherit the consulted trigger's prompt or pipeline.
    await waitFor(() => {
      expect(screen.getByPlaceholderText(/free-text prompt/i)).toHaveValue("");
    });
    expect((screen.getByTestId("pipeline-select") as HTMLSelectElement).value).not.toBe("p1");
  });
});
