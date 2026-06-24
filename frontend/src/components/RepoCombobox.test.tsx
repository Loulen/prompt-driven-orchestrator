import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import RepoCombobox from "./RepoCombobox";
import type { BrowseResponse } from "../api";
import { browseRepos } from "../api";

vi.mock("../api", () => ({
  browseRepos: vi.fn(),
}));

const mockedBrowse = vi.mocked(browseRepos);

const ROOT_RESPONSE: BrowseResponse = {
  path: "/home/user/projects",
  parent: "/home/user",
  entries: [
    { name: "alpha", path: "/home/user/projects/alpha", is_git_repo: true, is_symlink: false },
    { name: "beta", path: "/home/user/projects/beta", is_git_repo: false, is_symlink: false },
    {
      name: "zeta-link",
      path: "/home/user/projects/zeta-link",
      is_git_repo: false,
      is_symlink: true,
    },
  ],
  truncated: false,
  error: null,
};

function renderCombobox(overrides: Partial<Parameters<typeof RepoCombobox>[0]> = {}) {
  const props = {
    value: "",
    onChange: vi.fn(),
    recentRepos: [],
    repoValid: null,
    repoValidating: false,
    repoError: null,
    borderClass: "",
    ...overrides,
  };
  render(<RepoCombobox {...props} />);
  return props;
}

beforeEach(() => {
  mockedBrowse.mockReset();
  mockedBrowse.mockResolvedValue(ROOT_RESPONSE);
});

describe("RepoCombobox filesystem explorer (#131)", () => {
  it("renders the loupe trigger", () => {
    renderCombobox();
    expect(screen.getByTestId("repo-browse-trigger")).toBeInTheDocument();
  });

  it("opens the explorer on loupe click and lists the entries", async () => {
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));

    expect(await screen.findByTestId("repo-browser-modal")).toBeInTheDocument();
    const rows = await screen.findAllByTestId("repo-browse-entry");
    expect(rows).toHaveLength(3);
    // alpha-project carries the git indicator, only one of the three.
    expect(screen.getAllByTestId("repo-browse-git-dot")).toHaveLength(1);
    // zeta-link carries the symlink marker, only one of the three.
    expect(screen.getAllByTestId("repo-browse-symlink")).toHaveLength(1);
    // Breadcrumb shows the listed directory.
    expect(screen.getByTestId("repo-browse-path")).toHaveTextContent("/home/user/projects");
  });

  it("opens at the current absolute value (Option B)", async () => {
    renderCombobox({ value: "/abs/repo/path" });
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await waitFor(() => expect(mockedBrowse).toHaveBeenCalledWith("/abs/repo/path"));
  });

  it("opens at the backend default when the value is not an absolute path", async () => {
    renderCombobox({ value: "not-absolute" });
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await waitFor(() => expect(mockedBrowse).toHaveBeenCalledWith(undefined));
  });

  it("navigates into a clicked entry", async () => {
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    const rows = await screen.findAllByTestId("repo-browse-entry");
    fireEvent.click(rows[0]); // alpha
    await waitFor(() =>
      expect(mockedBrowse).toHaveBeenCalledWith("/home/user/projects/alpha"),
    );
  });

  it("navigates up to the parent when up is clicked", async () => {
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await screen.findByTestId("repo-browser-modal");
    fireEvent.click(screen.getByTestId("repo-browse-up"));
    await waitFor(() => expect(mockedBrowse).toHaveBeenCalledWith("/home/user"));
  });

  it("disables the up affordance at the filesystem root (parent null)", async () => {
    mockedBrowse.mockResolvedValue({ ...ROOT_RESPONSE, path: "/", parent: null });
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await screen.findByTestId("repo-browser-modal");
    await waitFor(() => expect(screen.getByTestId("repo-browse-up")).toBeDisabled());
  });

  it("picks the current directory through onChange and closes", async () => {
    const props = renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await screen.findByTestId("repo-browser-modal");
    fireEvent.click(screen.getByTestId("repo-browse-select"));
    expect(props.onChange).toHaveBeenCalledWith("/home/user/projects");
    await waitFor(() =>
      expect(screen.queryByTestId("repo-browser-modal")).not.toBeInTheDocument(),
    );
  });

  it("closes only the explorer on backdrop click", async () => {
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await screen.findByTestId("repo-browser-modal");
    fireEvent.click(screen.getByTestId("repo-browse-backdrop"));
    await waitFor(() =>
      expect(screen.queryByTestId("repo-browser-modal")).not.toBeInTheDocument(),
    );
  });

  it("closes the explorer on Escape", async () => {
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    await screen.findByTestId("repo-browser-modal");
    fireEvent.keyDown(document, { key: "Escape" });
    await waitFor(() =>
      expect(screen.queryByTestId("repo-browser-modal")).not.toBeInTheDocument(),
    );
  });

  it("surfaces an in-body error inline and keeps the breadcrumb", async () => {
    mockedBrowse.mockResolvedValue({
      path: "/home/user/projects/noaccess",
      parent: "/home/user/projects",
      entries: [],
      truncated: false,
      error: "permission denied: /home/user/projects/noaccess",
    });
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    expect(await screen.findByTestId("repo-browse-error")).toHaveTextContent(
      "permission denied",
    );
    // Breadcrumb is preserved — the user is not stranded on a blank pane.
    expect(screen.getByTestId("repo-browse-path")).toHaveTextContent(
      "/home/user/projects/noaccess",
    );
  });

  it("shows a truncation note when the listing is capped", async () => {
    mockedBrowse.mockResolvedValue({ ...ROOT_RESPONSE, truncated: true });
    renderCombobox();
    fireEvent.click(screen.getByTestId("repo-browse-trigger"));
    expect(await screen.findByText(/Showing first 1000/)).toBeInTheDocument();
  });
});
