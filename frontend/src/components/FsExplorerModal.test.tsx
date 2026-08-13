import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
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

// A11y (#437). Renders a focusable element OUTSIDE the modal: without it the Tab would
// loop through the card anyway and the trap test would be a false positive.
function renderWithOutside(overrides: Partial<Parameters<typeof FsExplorerModal>[0]> = {}) {
  const props = {
    onPick: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
  render(
    <>
      <button data-testid="outside">outside</button>
      <FsExplorerModal {...props} />
    </>,
  );
  return props;
}

// Harness for focus restoration: a trigger button that mounts the modal on click, so
// `document.activeElement` at mount is the trigger (what the restoration effect captures).
function FocusHarness() {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button data-testid="trigger" onClick={() => setOpen(true)}>
        open
      </button>
      {open && <FsExplorerModal onPick={vi.fn()} onClose={() => setOpen(false)} />}
    </>
  );
}

describe("FsExplorerModal — accessibility (#437)", () => {
  it("marks the card as an aria-modal dialog", async () => {
    renderModal();
    const modal = await screen.findByTestId("fs-browse-modal");
    expect(modal).toHaveAttribute("role", "dialog");
    expect(modal).toHaveAttribute("aria-modal", "true");
  });

  it("names the dialog from the title row when a title is given", async () => {
    renderModal({ mode: "file", title: "Choose a Dockerfile" });
    await screen.findByTestId("fs-browse-modal");
    expect(screen.getByRole("dialog")).toHaveAccessibleName("Choose a Dockerfile");
  });

  it("derives the name from mode when no title — dir mode says 'Choose a folder'", async () => {
    renderModal();
    await screen.findByTestId("fs-browse-modal");
    expect(screen.getByRole("dialog")).toHaveAccessibleName("Choose a folder");
  });

  it("derives the name from mode when no title — file mode says 'Choose a file'", async () => {
    renderModal({ mode: "file" });
    await screen.findByTestId("fs-browse-modal");
    expect(screen.getByRole("dialog")).toHaveAccessibleName("Choose a file");
  });

  it("moves initial focus to the dialog card", async () => {
    renderModal();
    await screen.findByTestId("fs-browse-modal");
    await waitFor(() => expect(screen.getByTestId("fs-browse-modal")).toHaveFocus());
  });

  it("traps Tab: from the last focusable it wraps to the first, never escaping", async () => {
    const user = userEvent.setup();
    renderWithOutside();
    await screen.findByTestId("fs-browse-select");
    // Wait for the load so 'Up' is enabled (parent "/") — it is the first focusable.
    await waitFor(() => expect(screen.getByTestId("fs-browse-up")).toBeEnabled());
    screen.getByTestId("fs-browse-select").focus(); // last focusable
    await user.tab();
    expect(screen.getByTestId("fs-browse-up")).toHaveFocus();
    expect(screen.getByTestId("outside")).not.toHaveFocus();
  });

  it("traps Shift+Tab: from the first focusable it wraps to the last, never escaping", async () => {
    const user = userEvent.setup();
    renderWithOutside();
    await screen.findByTestId("fs-browse-select");
    await waitFor(() => expect(screen.getByTestId("fs-browse-up")).toBeEnabled());
    screen.getByTestId("fs-browse-up").focus(); // first focusable
    await user.tab({ shift: true });
    expect(screen.getByTestId("fs-browse-select")).toHaveFocus();
    expect(screen.getByTestId("outside")).not.toHaveFocus();
  });

  it.each(["cancel", "escape", "confirm"] as const)(
    "restores focus to the opener on close via %s",
    async (closeVia) => {
      const user = userEvent.setup();
      render(<FocusHarness />);
      const trigger = screen.getByTestId("trigger");
      await user.click(trigger);
      // Open: the card holds focus; the confirm button is enabled once the load lands.
      await waitFor(() => expect(screen.getByTestId("fs-browse-modal")).toHaveFocus());
      await waitFor(() => expect(screen.getByTestId("fs-browse-select")).toBeEnabled());

      if (closeVia === "cancel") {
        await user.click(screen.getByRole("button", { name: "Cancel" }));
      } else if (closeVia === "escape") {
        fireEvent.keyDown(document, { key: "Escape" });
      } else {
        await user.click(screen.getByTestId("fs-browse-select"));
      }

      await waitFor(() => expect(trigger).toHaveFocus());
    },
  );

  it("does not swallow Escape — the trap effect ignores non-Tab keys", async () => {
    const props = renderModal();
    await screen.findByTestId("fs-browse-modal");
    fireEvent.keyDown(document, { key: "Escape" });
    expect(props.onClose).toHaveBeenCalled();
  });
});
