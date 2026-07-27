import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import FsExplorerModal from "./FsExplorerModal";
import type { BrowseResponse } from "../api";
import { browseFs } from "../api";

vi.mock("../api", () => ({
  browseFs: vi.fn(),
}));

const mockedBrowse = vi.mocked(browseFs);

const MIXED: BrowseResponse = {
  path: "/home/user",
  parent: "/",
  entries: [
    {
      name: "docker",
      path: "/home/user/docker",
      is_git_repo: false,
      is_symlink: false,
      is_dir: true,
    },
    {
      name: "notes.txt",
      path: "/home/user/notes.txt",
      is_git_repo: false,
      is_symlink: false,
      is_dir: false,
    },
  ],
  truncated: false,
  error: null,
};

function renderModal(overrides: Partial<Parameters<typeof FsExplorerModal>[0]> = {}) {
  const props = {
    onPick: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(<FsExplorerModal {...props} />);
  return props;
}

beforeEach(() => {
  mockedBrowse.mockReset();
  mockedBrowse.mockResolvedValue(MIXED);
});

describe("FsExplorerModal — call arity (#431 D6)", () => {
  it("calls browseFs with EXACTLY ONE argument in default mode", async () => {
    renderModal();
    // Asserting the RECORDED argument array (not just `toHaveBeenCalledWith`) is the
    // point: vitest compares arity strictly, so a trailing `undefined` or a `{}` would
    // break the frozen `RepoCombobox.test.tsx` assertions. This test makes that
    // regression impossible to land silently.
    await waitFor(() => expect(mockedBrowse).toHaveBeenCalled());
    expect(mockedBrowse.mock.calls[0]).toEqual([undefined]);
  });

  it("passes startPath as the sole argument in default mode", async () => {
    renderModal({ startPath: "/abs/start" });
    await waitFor(() => expect(mockedBrowse).toHaveBeenCalled());
    expect(mockedBrowse.mock.calls[0]).toEqual(["/abs/start"]);
  });

  it("sends both flags in file mode", async () => {
    renderModal({ mode: "file" });
    await waitFor(() =>
      expect(mockedBrowse).toHaveBeenCalledWith(undefined, { files: true, hidden: false }),
    );
  });

  it("sends hidden alone when showHidden is set in dir mode", async () => {
    renderModal({ showHidden: true });
    await waitFor(() =>
      expect(mockedBrowse).toHaveBeenCalledWith(undefined, { files: false, hidden: true }),
    );
  });
});

describe("FsExplorerModal — dir mode (the RepoCombobox contract)", () => {
  it("lists the entries and shows the breadcrumb", async () => {
    renderModal();
    expect(await screen.findByTestId("fs-browse-modal")).toBeInTheDocument();
    expect(await screen.findAllByTestId("fs-browse-entry")).toHaveLength(2);
    expect(screen.getByTestId("fs-browse-path")).toHaveTextContent("/home/user");
  });

  it("navigates on ANY row click — is_dir is never consulted in dir mode", async () => {
    // The dir-mode listing is dirs-only by contract, so even a row whose fixture says
    // `is_dir: false` must navigate. This is what keeps the frozen RepoCombobox tests
    // independent of a field their fixtures predate.
    renderModal();
    const rows = await screen.findAllByTestId("fs-browse-entry");
    fireEvent.click(rows[1]); // notes.txt, is_dir: false
    await waitFor(() => expect(mockedBrowse).toHaveBeenCalledWith("/home/user/notes.txt"));
  });

  it("picks the current directory and closes", async () => {
    const props = renderModal();
    await screen.findByTestId("fs-browse-modal");
    await waitFor(() => expect(screen.getByTestId("fs-browse-path")).toHaveTextContent("/home/user"));
    fireEvent.click(screen.getByTestId("fs-browse-select"));
    // Synchronous from the click handler (no waitFor) — RepoCombobox.test.tsx relies on it.
    expect(props.onPick).toHaveBeenCalledWith("/home/user");
    expect(props.onClose).toHaveBeenCalled();
  });

  it("labels the confirm button 'Select this folder'", async () => {
    renderModal();
    expect(await screen.findByTestId("fs-browse-select")).toHaveTextContent("Select this folder");
  });

  it("says 'folders' in the truncation note, still matching /Showing first 1000/", async () => {
    mockedBrowse.mockResolvedValue({ ...MIXED, truncated: true });
    renderModal();
    expect(await screen.findByText(/Showing first 1000/)).toHaveTextContent(
      "Showing first 1000 folders",
    );
  });
});

describe("FsExplorerModal — file mode (the Dockerfile picker)", () => {
  it("selects a file row instead of navigating, and keeps the modal open", async () => {
    const props = renderModal({ mode: "file" });
    const rows = await screen.findAllByTestId("fs-browse-entry");
    mockedBrowse.mockClear();
    fireEvent.click(rows[1]); // notes.txt
    expect(mockedBrowse).not.toHaveBeenCalled();
    expect(props.onPick).not.toHaveBeenCalled();
    expect(screen.getByTestId("fs-browse-modal")).toBeInTheDocument();
  });

  it("still navigates on a directory row", async () => {
    renderModal({ mode: "file" });
    const rows = await screen.findAllByTestId("fs-browse-entry");
    fireEvent.click(rows[0]); // docker/
    await waitFor(() =>
      expect(mockedBrowse).toHaveBeenCalledWith("/home/user/docker", {
        files: true,
        hidden: false,
      }),
    );
  });

  it("disables confirm until a file is selected, then picks it", async () => {
    const props = renderModal({ mode: "file" });
    const confirm = await screen.findByTestId("fs-browse-select");
    expect(confirm).toBeDisabled();

    const rows = await screen.findAllByTestId("fs-browse-entry");
    fireEvent.click(rows[1]);
    await waitFor(() => expect(screen.getByTestId("fs-browse-select")).toBeEnabled());

    fireEvent.click(screen.getByTestId("fs-browse-select"));
    expect(props.onPick).toHaveBeenCalledWith("/home/user/notes.txt");
    expect(props.onClose).toHaveBeenCalled();
  });

  it("labels the confirm button 'Select this file' by default", async () => {
    renderModal({ mode: "file" });
    expect(await screen.findByTestId("fs-browse-select")).toHaveTextContent("Select this file");
  });

  it("honours confirmLabel and title", async () => {
    renderModal({
      mode: "file",
      confirmLabel: "Use this Dockerfile",
      title: "Choose a Dockerfile",
    });
    expect(await screen.findByTestId("fs-browse-select")).toHaveTextContent(
      "Use this Dockerfile",
    );
    expect(screen.getByText("Choose a Dockerfile")).toBeInTheDocument();
  });

  it("renders no title row when title is omitted (keeps RepoCombobox pixel-identical)", async () => {
    renderModal();
    await screen.findByTestId("fs-browse-modal");
    expect(screen.queryByText("Choose a Dockerfile")).not.toBeInTheDocument();
  });

  it("says 'entries' in the truncation note", async () => {
    mockedBrowse.mockResolvedValue({ ...MIXED, truncated: true });
    renderModal({ mode: "file" });
    expect(await screen.findByText(/Showing first 1000/)).toHaveTextContent(
      "Showing first 1000 entries",
    );
  });
});

describe("FsExplorerModal — testids, layering, errors", () => {
  it("honours testIdPrefix for every namespaced testid", async () => {
    renderModal({ testIdPrefix: "repo-browse" });
    expect(await screen.findByTestId("repo-browse-modal")).toBeInTheDocument();
    expect(screen.getByTestId("repo-browse-backdrop")).toBeInTheDocument();
    expect(screen.getByTestId("repo-browse-up")).toBeInTheDocument();
    expect(screen.getByTestId("repo-browse-path")).toBeInTheDocument();
    expect(screen.getByTestId("repo-browse-select")).toBeInTheDocument();
    expect(await screen.findAllByTestId("repo-browse-entry")).toHaveLength(2);
  });

  it("honours modalTestId for the container ALONE (the #131 irregular name)", async () => {
    renderModal({ testIdPrefix: "repo-browse", modalTestId: "repo-browser-modal" });
    expect(await screen.findByTestId("repo-browser-modal")).toBeInTheDocument();
    // The rest of the namespace still follows the prefix.
    expect(screen.getByTestId("repo-browse-backdrop")).toBeInTheDocument();
    expect(screen.queryByTestId("repo-browse-modal")).not.toBeInTheDocument();
  });

  it("closes on backdrop click and on Escape", async () => {
    const props = renderModal();
    await screen.findByTestId("fs-browse-modal");
    fireEvent.click(screen.getByTestId("fs-browse-backdrop"));
    expect(props.onClose).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(document, { key: "Escape" });
    expect(props.onClose).toHaveBeenCalledTimes(2);
  });

  it("disables the up affordance at the filesystem root (parent null)", async () => {
    mockedBrowse.mockResolvedValue({ ...MIXED, path: "/", parent: null });
    renderModal();
    await screen.findByTestId("fs-browse-modal");
    await waitFor(() => expect(screen.getByTestId("fs-browse-up")).toBeDisabled());
  });

  it("surfaces an in-body error inline and keeps the breadcrumb", async () => {
    mockedBrowse.mockResolvedValue({
      path: "/home/user/noaccess",
      parent: "/home/user",
      entries: [],
      truncated: false,
      error: "permission denied: /home/user/noaccess",
    });
    renderModal();
    expect(await screen.findByTestId("fs-browse-error")).toHaveTextContent("permission denied");
    expect(screen.getByTestId("fs-browse-path")).toHaveTextContent("/home/user/noaccess");
  });

  it("surfaces a thrown error too (400/500, not the in-body shape)", async () => {
    mockedBrowse.mockRejectedValue(new Error("path must be an absolute path"));
    renderModal();
    expect(await screen.findByTestId("fs-browse-error")).toHaveTextContent(
      "path must be an absolute path",
    );
  });
});
