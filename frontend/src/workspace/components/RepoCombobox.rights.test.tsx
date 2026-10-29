import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RepoCombobox from "./RepoCombobox";

vi.mock("../../settings/api/settings", () => ({}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));

function renderCombobox(overrides: Partial<Parameters<typeof RepoCombobox>[0]> = {}) {
  const props = {
    value: "",
    onChange: vi.fn(),
    recentRepos: ["/home/bob/recent"],
    repoValid: null,
    repoValidating: false,
    repoError: null,
    ...overrides,
  };
  render(<RepoCombobox {...props} />);
  return props;
}

const GROUPS = [
  { label: "PDO", repos: ["/home/bob/code/PDO-v2", "/home/bob/code/pdo-docs"] },
  { label: "pdo-widgets", repos: ["/home/bob/code/pdo-widgets"] },
];

describe("RepoCombobox under the rights of the caller (CORE-5)", () => {
  it("keeps the loupe by default", () => {
    renderCombobox();
    expect(screen.getByTestId("repo-browse-trigger")).toBeInTheDocument();
  });

  it("hides the loupe when the disk may not be browsed, with a line of help", () => {
    renderCombobox({ canBrowse: false, help: "A new repository goes through an administrator." });
    expect(screen.queryByTestId("repo-browse-trigger")).toBeNull();
    expect(screen.getByTestId("repo-help")).toHaveTextContent(
      "A new repository goes through an administrator.",
    );
  });

  it("offers the repositories grouped by Projet, and the hidden Projets' note", () => {
    const props = renderCombobox({
      canBrowse: false,
      groups: GROUPS,
      groupsNote: "1 Project(s) hidden: you can't create Runs there.",
      home: "/home/bob",
    });
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    const groups = screen.getAllByTestId("repo-group");
    expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual(["PDO", "pdo-widgets"]);
    const items = screen.getAllByTestId("repo-group-item");
    expect(items.map((i) => i.textContent)).toEqual([
      "~/code/PDO-v2",
      "~/code/pdo-docs",
      "~/code/pdo-widgets",
    ]);
    expect(screen.getByTestId("repo-groups-note")).toHaveTextContent(
      "1 Project(s) hidden: you can't create Runs there.",
    );
    // The recent repositories are not offered: the groups replace them.
    expect(screen.queryByTestId("recent-repos-dropdown")).toBeNull();
    fireEvent.mouseDown(items[2]);
    expect(props.onChange).toHaveBeenCalledWith("/home/bob/code/pdo-widgets");
  });

  it("narrows the groups to what is typed", () => {
    renderCombobox({ value: "docs", canBrowse: false, groups: GROUPS });
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.getAllByTestId("repo-group-item").map((i) => i.getAttribute("title"))).toEqual([
      "/home/bob/code/pdo-docs",
    ]);
  });
});
