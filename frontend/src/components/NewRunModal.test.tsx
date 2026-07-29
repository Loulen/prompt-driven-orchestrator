import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import NewRunModal from "./NewRunModal";
import { useEditStore } from "../stores/editStore";
import type { InstanceSettings, PipelineListEntry } from "../types";

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
  // #410: the modal fetches settings on open (default_sandbox prefill +
  // sandbox_docker greying). Default: off + Docker available. Tests override per case.
  fetchSettings: vi.fn().mockResolvedValue({
    session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
    reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
    guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
    default_model: { effective: null, source: "default", stored: null, env: null, default: null },
    default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
    sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
    // #432: the sandbox `<select>` options are DATA now — the two virtual defaults.
    sandbox_profiles: [
      { name: "full", virtual: true },
      { name: "minimal", virtual: true },
    ],
    home: "/home/user",
    updated_at: "2026-07-01T10:00:00.000Z",
  }),
  // #431 prophylaxis: this file renders `RepoCombobox`, which mounts `FsExplorerModal`
  // on a loupe click. No test clicks it today, but a missing key here would throw at
  // FIRST ACCESS (`No "browseFs" export is defined`), not at import — a trap worth
  // disarming rather than rediscovering.
  browseFs: vi.fn().mockResolvedValue({
    path: "/home/user",
    parent: "/",
    entries: [],
    truncated: false,
    error: null,
  }),
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

const { validateRepo, listBranches, createRun, createTrigger, updateTrigger, fetchPipelines, fetchSettings, promotePipeline, testGuard } = await import("../api");

const noop = () => {};

beforeEach(() => {
  vi.clearAllMocks();
  // `clearAllMocks` wipes recorded calls but KEEPS implementations, so a
  // `mockResolvedValue` set inside one test leaks into every later one. The
  // repo-switch tests (#454) re-point this mock per repo, so restore the
  // documented default here rather than trusting each test to clean up.
  vi.mocked(listBranches).mockResolvedValue(["main", "dev", "feature-x"]);
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

  /**
   * #454: the `!sourceBranch` guard blocked re-selection when the repo changed.
   * The `<select>` then DISPLAYED the new repo's only option while the state
   * still held a branch that repo does not have → launch refused with
   * `branch 'main' does not exist`. Same shows-one-sends-another family as #452.
   */
  describe("changing the target repo re-selects the source branch (#454)", () => {
    /**
     * Asserting `branchSelect.value` CANNOT catch this bug, and that is the whole
     * point of it: a `<select>` whose React value matches none of its options
     * reports the FIRST option's value. So the DOM read `master` even while the
     * state held `main` — the read that looks like a check and passes either way.
     * Only what the form actually submits distinguishes the two.
     */
    it("launches against the re-selected branch, not the stale one", async () => {
      vi.mocked(fetchPipelines).mockResolvedValue([
        makePipeline({ id: "p1", name: "P1", scope: "repo" }),
      ]);
      vi.mocked(listBranches).mockResolvedValue(["main", "dev", "feature-x"]);
      renderModal();
      await enterValidRepo("/home/user/project-a"); // → main

      vi.mocked(listBranches).mockResolvedValue(["master"]);
      await enterValidRepo("/home/user/project-b");

      const branchSelect = screen.getByLabelText(/source branch/i) as HTMLSelectElement;
      await waitFor(() => {
        expect(Array.from(branchSelect.options).map((o) => o.value)).toEqual(["master"]);
      });

      fireEvent.change(screen.getByTestId("pipeline-select"), { target: { value: "p1" } });
      fireEvent.change(screen.getByPlaceholderText(/free-text prompt/i), {
        target: { value: "do the thing" },
      });

      vi.useRealTimers();
      fireEvent.click(screen.getByRole("button", { name: /launch/i }));

      await waitFor(() => {
        // Pre-fix this was `main`, a branch project-b does not have → the daemon
        // refused the launch with `branch 'main' does not exist`.
        expect(createRun).toHaveBeenCalledWith(
          expect.objectContaining({ source_branch: "master" }),
        );
      });
    });

    /**
     * Guard against over-correcting: resetting unconditionally on every repo
     * change would throw away a deliberate choice that the new repo still honours.
     * This one passes before the fix too — it is here to keep the fix honest, not
     * to prove the bug.
     */
    it("keeps the user's branch when the new repo still has it", async () => {
      vi.mocked(fetchPipelines).mockResolvedValue([
        makePipeline({ id: "p1", name: "P1", scope: "repo" }),
      ]);
      vi.mocked(listBranches).mockResolvedValue(["main", "dev", "feature-x"]);
      renderModal();
      await enterValidRepo("/home/user/project-a");

      const branchSelect = screen.getByLabelText(/source branch/i) as HTMLSelectElement;
      fireEvent.change(branchSelect, { target: { value: "feature-x" } });
      expect(branchSelect.value).toBe("feature-x");

      vi.mocked(listBranches).mockResolvedValue(["main", "feature-x"]);
      await enterValidRepo("/home/user/project-b");

      fireEvent.change(screen.getByTestId("pipeline-select"), { target: { value: "p1" } });
      fireEvent.change(screen.getByPlaceholderText(/free-text prompt/i), {
        target: { value: "do the thing" },
      });

      vi.useRealTimers();
      fireEvent.click(screen.getByRole("button", { name: /launch/i }));

      await waitFor(() => {
        expect(createRun).toHaveBeenCalledWith(
          expect.objectContaining({ source_branch: "feature-x" }),
        );
      });
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

describe("NewRunModal — sandbox selector (#410)", () => {
  function settingsFixture(overrides: Partial<InstanceSettings> = {}): InstanceSettings {
    return {
      session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
      reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
      guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
      default_model: { effective: null, source: "default", stored: null, env: null, default: null },
      default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
      // #431: required fields on InstanceSettings; this modal reads neither, they are
      // here to satisfy the typed fixture.
      sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
      // #432: the `<select>` options come from here. Both virtual defaults, no row.
      sandbox_profiles: [
        { name: "full", virtual: true },
        { name: "minimal", virtual: true },
      ],
      home: "/home/user",
      updated_at: "2026-07-01T10:00:00.000Z",
      ...overrides,
    };
  }

  /**
   * #452 replaces #410's prefill. The run selector NAMES the instance default instead of
   * copying it into the field: the value stays the `""` sentinel, so the key is omitted and
   * the daemon resolves. See the `#452` describe below for why copying it was unsound.
   */
  it("names the instance default on the inherit option without seeding the field", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(
      settingsFixture({
        default_sandbox: { effective: "full", source: "stored", stored: "full", env: null, default: "off", reason: null },
      }),
    );
    vi.mocked(fetchPipelines).mockResolvedValue([makePipeline({ id: "p1", name: "P", scope: "repo" })]);
    renderModal();
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => {
      expect(Array.from(select.options).find((o) => o.value === "")).toHaveTextContent(
        "Use instance default (full)",
      );
    });
    // The field asserts nothing of its own.
    expect(select.value).toBe("");
  });

  it("disables full/minimal and BLOCKS the launch when Docker is unavailable (no silent clamp to off)", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(
      settingsFixture({
        // The instance default is `minimal`, but Docker is down.
        default_sandbox: { effective: "minimal", source: "stored", stored: "minimal", env: null, default: "off", reason: null },
        sandbox_docker: { available: false, reason: "Docker daemon unreachable", checked_at: "x" },
      }),
    );
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "P", scope: "repo", prompt_required: false }),
    ]);
    renderModal();
    await enterValidRepo();

    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    // #452: NOT clamped to `off` — the field still says "I did not choose".
    await waitFor(() => expect(screen.getByTestId("sandbox-doomed-warning")).toBeInTheDocument());
    expect(select.value).toBe("");
    // full/minimal options are disabled; the reason is surfaced.
    const options = Array.from(select.options);
    expect(options.find((o) => o.value === "full")?.disabled).toBe(true);
    expect(options.find((o) => o.value === "minimal")?.disabled).toBe(true);
    expect(screen.getByTestId("sandbox-docker-warning")).toHaveTextContent(/unreachable/i);

    // The Run doomed to a RunFailed is still prevented — by refusing it, not by
    // answering `off` on the user's behalf.
    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("launch-button")).toBeDisabled());
    fireEvent.click(screen.getByTestId("launch-button"));
    expect(createRun).not.toHaveBeenCalled();

    // Demoting to `off` is the user's call, and it unblocks.
    fireEvent.change(select, { target: { value: "off" } });
    await waitFor(() => expect(screen.getByTestId("launch-button")).toBeEnabled());
    expect(screen.queryByTestId("sandbox-doomed-warning")).not.toBeInTheDocument();
  });

  it("passes the chosen sandbox mode to createRun", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(
      settingsFixture({
        default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
      }),
    );
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Optional Pipeline", scope: "repo", prompt_required: false }),
    ]);
    renderModal();
    await enterValidRepo();

    // An explicit pick — the case this test is named for. It must survive as-is, even
    // though it disagrees with the instance default (`off`).
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(Array.from(select.options).some((o) => o.value === "full")).toBe(true));
    fireEvent.change(select, { target: { value: "full" } });

    vi.useRealTimers();
    await waitFor(() => expect(screen.getByRole("button", { name: /launch/i })).toBeEnabled());
    fireEvent.click(screen.getByRole("button", { name: /launch/i }));

    await waitFor(() => {
      expect(createRun).toHaveBeenCalledWith(expect.objectContaining({ sandbox: "full" }));
    });
  });

  it("offers a 'use instance default' option in trigger mode and sends null when picked", async () => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Auditor", scope: "repo", prompt_required: false }),
    ]);
    // A trigger whose sandbox is set to `minimal` — prefills the select to `minimal`.
    const trigger = {
      id: "trg-sbx",
      name: "Nightly",
      pipeline_id: "p1",
      pipeline_name: "Auditor",
      target_repo: "/home/user/project",
      source_branch: "dev",
      input_template: "audit",
      variables: {},
      cron: "0 9 * * *",
      guard_command: null,
      overlap_policy: "skip",
      sandbox: "minimal",
      enabled: true,
      next_fire_at: null,
      last_fired_at: null,
      last_outcome: null,
    };
    render(
      <NewRunModal open={true} onClose={noop} onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger }} />,
    );

    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(select.value).toBe("minimal"));
    // Trigger mode exposes the inherit option.
    expect(Array.from(select.options).some((o) => o.value === "")).toBe(true);

    // Repo validation must resolve so Save is enabled.
    await vi.advanceTimersByTimeAsync(500);
    await waitFor(() => expect(validateRepo).toHaveBeenCalledWith("/home/user/project"));

    // Reset to "use instance default" → the PATCH must clear it (null).
    fireEvent.change(select, { target: { value: "" } });
    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("save-trigger-button")).toBeEnabled());
    fireEvent.click(screen.getByTestId("save-trigger-button"));

    await waitFor(() => {
      expect(updateTrigger).toHaveBeenCalledWith(
        "trg-sbx",
        expect.objectContaining({ sandbox: null }),
      );
    });
  });

  // -- #432: the options are DATA, and a vanished profile is a tombstone -------

  it("lists off plus every staging profile the daemon serves", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(
      settingsFixture({
        sandbox_profiles: [
          { name: "full", virtual: true },
          { name: "full-no-mcp", virtual: false },
          { name: "minimal", virtual: true },
        ],
      }),
    );
    vi.mocked(fetchPipelines).mockResolvedValue([makePipeline({ id: "p1", name: "P", scope: "repo" })]);
    renderModal();
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() =>
      // #452: the inherit sentinel leads the list in run mode too.
      expect(Array.from(select.options).map((o) => o.value)).toEqual([
        "",
        "off",
        "full",
        "full-no-mcp",
        "minimal",
      ]),
    );
  });

  /**
   * THE PHANTOM-PROFILE RULE (#432). A trigger whose stored profile has been deleted keeps
   * a tombstone option and blocks Save.
   *
   * Without it React sets `selectedIndex = -1`, the field renders blank, and saving would
   * PATCH `sandbox: null` — a SILENT FALLBACK to the instance default, exactly what
   * ADR-0031 §7 forbids. Deliberately separate from the Docker clamp: clamping to `off` is
   * legitimate for an unavailable Docker, and would be a silent demotion here.
   */
  it("tombstones a trigger's vanished profile and blocks the save", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture());
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Auditor", scope: "repo", prompt_required: false }),
    ]);
    const trigger = {
      id: "trg-gone",
      name: "Nightly",
      pipeline_id: "p1",
      pipeline_name: "Auditor",
      target_repo: "/home/user/project",
      source_branch: "dev",
      input_template: "audit",
      variables: {},
      cron: "0 9 * * *",
      guard_command: null,
      overlap_policy: "skip",
      // Materialised once, deleted since — the only way to get a dangling reference.
      sandbox: "full-no-mcp",
      enabled: true,
      next_fire_at: null,
      last_fired_at: null,
      last_outcome: null,
    };
    render(
      <NewRunModal open={true} onClose={noop} onCreated={noop}
        openIntent={{ kind: "edit-trigger", trigger }} />,
    );

    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    // The seeded value is NEVER rewritten: still selected, and visibly a tombstone.
    await waitFor(() => expect(select.value).toBe("full-no-mcp"));
    expect(screen.getByTestId("sandbox-missing-profile")).toBeInTheDocument();
    expect(screen.getByTestId("sandbox-missing-profile-warning")).toHaveTextContent(
      /does not fall back to a default/i,
    );

    vi.useRealTimers();
    await waitFor(() =>
      expect(screen.getByRole("button", { name: /save trigger/i })).toBeDisabled(),
    );
    expect(updateTrigger).not.toHaveBeenCalled();
  });

  it("does not tombstone `off`, which is never a profile", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture());
    vi.mocked(fetchPipelines).mockResolvedValue([makePipeline({ id: "p1", name: "P", scope: "repo" })]);
    renderModal();
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(Array.from(select.options).length).toBeGreaterThan(1));
    fireEvent.change(select, { target: { value: "off" } });
    expect(select.value).toBe("off");
    expect(screen.queryByTestId("sandbox-missing-profile")).not.toBeInTheDocument();
  });

  it("does not tombstone the inherit sentinel either", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture());
    vi.mocked(fetchPipelines).mockResolvedValue([makePipeline({ id: "p1", name: "P", scope: "repo" })]);
    renderModal();
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(Array.from(select.options).length).toBeGreaterThan(1));
    expect(select.value).toBe("");
    expect(screen.queryByTestId("sandbox-missing-profile")).not.toBeInTheDocument();
  });
});

/**
 * #452 — `default_sandbox` must be REACHABLE from the launch dialog.
 *
 * The daemon's contract is `Option<SandboxMode>`: `None` = "defer to the instance default",
 * `Some(Off)` = "run on the host, and nothing may override that upward". #410 seeded the run
 * selector by copying the resolved default into the field, asynchronously and best-effort, and
 * `sandbox: sandbox || undefined` omits the key only for `""` — which run mode never held. So
 * the key was structurally always present, and every way the prefill could miss its window
 * landed on an explicit `off`: a choice the user never made, in the least protective direction,
 * that nothing downstream could undo.
 *
 * The tests below pin the three ways it missed. Each one FAILS on the pre-fix component (it
 * posts `sandbox: "off"`), which is the point: the happy path at
 * "names the instance default…" passed throughout and hid all three.
 */
describe("NewRunModal — the launch dialog can defer to default_sandbox (#452)", () => {
  function settingsFixture(overrides: Partial<InstanceSettings> = {}): InstanceSettings {
    return {
      session_cap: { effective: 20, source: "default", stored: null, env: null, default: 20 },
      reaper_ttl_secs: { effective: 3600, source: "default", stored: null, env: null, default: 3600 },
      guard_timeout_secs: { effective: 60, source: "default", stored: null, env: null, default: 60 },
      default_model: { effective: null, source: "default", stored: null, env: null, default: null },
      default_sandbox: { effective: "off", source: "default", stored: null, env: null, default: "off", reason: null },
      sandbox_docker: { available: true, reason: null, checked_at: "2026-07-01T10:00:00.000Z" },
      sandbox_profiles: [
        { name: "full", virtual: true },
        { name: "minimal", virtual: true },
      ],
      home: "/home/user",
      updated_at: "2026-07-01T10:00:00.000Z",
      ...overrides,
    };
  }

  const defaultIs = (name: string): Partial<InstanceSettings> => ({
    default_sandbox: { effective: name, source: "stored", stored: name, env: null, default: "off", reason: null },
  });

  /**
   * "Omitted" is read at the `createRun` boundary rather than on the wire: `api.ts` skips a
   * falsy `sandbox` on BOTH transports (`JSON.stringify` drops `undefined`; the multipart path
   * guards with `if (req.sandbox)`), so `undefined` here IS an absent key there. Asserted as a
   * property lookup, not with `toHaveBeenCalledWith`, because `objectContaining` cannot express
   * absence — and because vitest compares arity strictly, so a spurious trailing argument would
   * fail the assertion for the wrong reason.
   */
  function sandboxSentTo(mock: typeof createRun) {
    const calls = vi.mocked(mock).mock.calls;
    expect(calls).toHaveLength(1);
    return calls[0][0].sandbox;
  }

  async function launchWithoutTouchingSandbox() {
    await enterValidRepo();
    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("launch-button")).toBeEnabled());
    fireEvent.click(screen.getByTestId("launch-button"));
    await waitFor(() => expect(createRun).toHaveBeenCalled());
  }

  beforeEach(() => {
    vi.mocked(fetchPipelines).mockResolvedValue([
      makePipeline({ id: "p1", name: "Optional Pipeline", scope: "repo", prompt_required: false }),
    ]);
  });

  it("omits the sandbox key when the user never touches the selector", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture(defaultIs("full")));
    renderModal();
    // Wait for the settings to land, so this is not accidentally the in-flight case below.
    await screen.findByTestId("sandbox-select");
    await waitFor(() => expect(fetchSettings).toHaveBeenCalled());

    await launchWithoutTouchingSandbox();

    // The whole issue in one line: never `off`, and never anything at all.
    expect(sandboxSentTo(createRun)).toBeUndefined();
  });

  /**
   * Mode B of the repro, and the only one that needs no fault injection. The modal is
   * always-mounted, so `settings` survives a close; the old seeding effect therefore ran
   * synchronously against the STALE value on reopen and latched its one-shot ref before the
   * fresh fetch could land. A user who changes `default_sandbox` in Settings and reopens New
   * Run — without reloading the page — got the previous default posted explicitly.
   */
  it("does not seed a reopened dialog from settings cached before the default changed", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture(defaultIs("off")));
    const { rerender } = render(
      <NewRunModal open={true} onClose={noop} onCreated={noop} openIntent={{ kind: "run" }} />,
    );
    await screen.findByTestId("sandbox-select");
    await waitFor(() => expect(fetchSettings).toHaveBeenCalledTimes(1));

    // Close. The component stays mounted, so `settings` stays in state.
    rerender(<NewRunModal open={false} onClose={noop} onCreated={noop} openIntent={{ kind: "run" }} />);

    // The operator switches the instance default to `full` in Settings.
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture(defaultIs("full")));

    rerender(<NewRunModal open={true} onClose={noop} onCreated={noop} openIntent={{ kind: "run" }} />);
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    expect(select.value).toBe("");
    // And the label catches up with the new default once the refetch lands.
    await waitFor(() =>
      expect(Array.from(select.options).find((o) => o.value === "")).toHaveTextContent(
        "Use instance default (full)",
      ),
    );

    await launchWithoutTouchingSandbox();
    expect(sandboxSentTo(createRun)).toBeUndefined();
  });

  /**
   * Mode C. `GET /settings` failing used to be swallowed whole: the option list silently
   * degraded to `off` alone — indistinguishable from an instance without Docker — and the
   * submission went ahead with an explicit `off`. Deferring is the right answer when we know
   * nothing: the daemon has the default, we do not.
   */
  it("still defers, and says so, when GET /settings fails", async () => {
    vi.mocked(fetchSettings).mockRejectedValue(new Error("daemon unreachable"));
    renderModal();
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(screen.getByTestId("sandbox-settings-error")).toBeInTheDocument());
    expect(select.value).toBe("");

    await launchWithoutTouchingSandbox();
    expect(sandboxSentTo(createRun)).toBeUndefined();
  });

  /**
   * The prefill also lost its race whenever the fetch was merely SLOW — a Run launched
   * before it resolved was posted with the `off` initial state. Nothing waits on the fetch
   * any more, so there is no race left to lose.
   */
  it("defers when the launch beats the settings fetch", async () => {
    let resolveSettings!: (s: InstanceSettings) => void;
    vi.mocked(fetchSettings).mockReturnValue(
      new Promise<InstanceSettings>((resolve) => {
        resolveSettings = resolve;
      }),
    );
    renderModal();
    await screen.findByTestId("sandbox-select");

    await launchWithoutTouchingSandbox();
    expect(sandboxSentTo(createRun)).toBeUndefined();

    resolveSettings(settingsFixture(defaultIs("full")));
  });

  it("sends an explicit off when the user actually picks it", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(settingsFixture(defaultIs("full")));
    renderModal();
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(Array.from(select.options).length).toBeGreaterThan(1));
    fireEvent.change(select, { target: { value: "off" } });

    await enterValidRepo();
    vi.useRealTimers();
    await waitFor(() => expect(screen.getByTestId("launch-button")).toBeEnabled());
    fireEvent.click(screen.getByTestId("launch-button"));
    await waitFor(() => expect(createRun).toHaveBeenCalled());

    // The sentinel must not swallow a real choice: `off` on purpose stays `off`, explicitly,
    // so it keeps winning over the instance default.
    expect(sandboxSentTo(createRun)).toBe("off");
  });

  it("surfaces a dangling instance default instead of letting the launch 400 unexplained", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(
      settingsFixture({
        default_sandbox: {
          effective: "deleted-profile",
          source: "stored",
          stored: "deleted-profile",
          env: null,
          default: "off",
          reason: "No staging profile named `deleted-profile`",
        },
      }),
    );
    renderModal();
    await screen.findByTestId("sandbox-select");
    await waitFor(() =>
      expect(screen.getByTestId("sandbox-default-reason")).toHaveTextContent(/deleted-profile/),
    );
  });

  /**
   * Trigger mode is untouched by all of this: a Trigger resolves its sandbox when it FIRES,
   * so today's Docker probe cannot condemn it and the inherit option stays unqualified.
   */
  it("leaves trigger mode's inherit option unqualified and unblocked", async () => {
    vi.mocked(fetchSettings).mockResolvedValue(
      settingsFixture({
        ...defaultIs("full"),
        sandbox_docker: { available: false, reason: "Docker daemon unreachable", checked_at: "x" },
      }),
    );
    render(
      <NewRunModal open={true} onClose={noop} onCreated={noop} openIntent={{ kind: "new-trigger" }} />,
    );
    const select = (await screen.findByTestId("sandbox-select")) as HTMLSelectElement;
    await waitFor(() => expect(screen.getByTestId("sandbox-docker-warning")).toBeInTheDocument());
    expect(select.value).toBe("");
    expect(Array.from(select.options).find((o) => o.value === "")).toHaveTextContent(
      "Use instance default",
    );
    expect(Array.from(select.options).find((o) => o.value === "")).not.toHaveTextContent("(full)");
    expect(screen.queryByTestId("sandbox-doomed-warning")).not.toBeInTheDocument();
  });
});
