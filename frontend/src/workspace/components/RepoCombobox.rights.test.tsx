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

describe("RepoCombobox under the rights of the caller (CORE-5, CORE-205)", () => {
  it("keeps the loupe by default", () => {
    renderCombobox();
    expect(screen.getByTestId("repo-browse-trigger")).toBeInTheDocument();
  });

  it("greys the loupe when the disk may not be browsed, saying why, with a line of help (CORE-205)", () => {
    renderCombobox({ browseRefused: "You can't browse the disk. Ask an administrator.", help: "A new repository goes through an administrator." });
    expect(screen.getByTestId("repo-browse-trigger")).toHaveAttribute("aria-disabled", "true");
    expect(screen.getByTestId("repo-help")).toHaveTextContent(
      "A new repository goes through an administrator.",
    );
  });

  it("offers the repositories grouped by Projet, and the hidden Projets' note", () => {
    const props = renderCombobox({
      browseRefused: "You can't browse the disk.",
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
    renderCombobox({ value: "docs", browseRefused: "You can't browse the disk.", groups: GROUPS });
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.getAllByTestId("repo-group-item").map((i) => i.getAttribute("title"))).toEqual([
      "/home/bob/code/pdo-docs",
    ]);
  });
});

describe("RepoCombobox searching only the Projets' repositories (no disk)", () => {
  const PROJECT_GROUPS = [
    { label: "Payments", projectId: "p1", repos: ["/home/bob/clones/payments-api", "/home/bob/clones/payments-web"] },
    { label: "Risk", projectId: "p2", repos: ["/home/bob/clones/payments-api", "/home/bob/clones/risk-engine"] },
  ];

  function renderSearch(overrides: Partial<Parameters<typeof RepoCombobox>[0]> = {}) {
    return renderCombobox({
      searchOnly: true,
      groups: PROJECT_GROUPS,
      browseRefused: "You can't browse the disk.",
      home: "/home/bob",
      onPick: vi.fn(),
      ...overrides,
    });
  }

  it("searches, with its placeholder and the search icon, the loupe greyed", () => {
    renderSearch();
    const input = screen.getByTestId("target-repo-input");
    expect(input).toHaveAttribute("placeholder", "Search your Projects' repositories");
    expect(screen.getByTestId("repo-search-icon")).toBeInTheDocument();
    expect(screen.getByTestId("repo-browse-trigger")).toHaveAttribute("aria-disabled", "true");
  });

  it("filters the list with what is typed, never taking it as the repository", () => {
    const props = renderSearch();
    const input = screen.getByTestId("target-repo-input");
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "pay" } });
    expect(props.onChange).not.toHaveBeenCalled();
    expect(screen.getAllByTestId("repo-group").map((g) => g.getAttribute("aria-label"))).toEqual(["Payments", "Risk"]);
    expect(screen.getAllByTestId("repo-group-item").map((i) => i.textContent)).toEqual([
      "~/clones/payments-api",
      "~/clones/payments-web",
      "~/clones/payments-api",
    ]);
    // No sentence closes the list.
    expect(screen.queryByTestId("repo-groups-note")).toBeNull();
  });

  it("picks a repository under the group it is listed in", () => {
    const props = renderSearch();
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    const risk = screen.getAllByTestId("repo-group")[1];
    fireEvent.mouseDown(risk.querySelector("[data-testid=repo-group-item]")!);
    expect(props.onPick).toHaveBeenCalledWith(
      "/home/bob/clones/payments-api",
      expect.objectContaining({ projectId: "p2", label: "Risk" }),
    );
    expect(props.onChange).not.toHaveBeenCalled();
  });

  it("says when nothing matches, and Enter on such a search does nothing, without an error", () => {
    const props = renderSearch();
    const input = screen.getByTestId("target-repo-input");
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "/tmp/elsewhere" } });
    expect(screen.getByTestId("repo-no-match")).toHaveTextContent("No repository matches.");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onPick).not.toHaveBeenCalled();
    expect(props.onChange).not.toHaveBeenCalled();
    expect(screen.queryByTestId("repo-error")).toBeNull();
    expect(input).not.toHaveAttribute("aria-invalid");
  });

  it("takes the first match on Enter", () => {
    const props = renderSearch();
    const input = screen.getByTestId("target-repo-input");
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "risk" } });
    fireEvent.keyDown(input, { key: "Enter" });
    expect(props.onPick).toHaveBeenCalledWith(
      "/home/bob/clones/risk-engine",
      expect.objectContaining({ projectId: "p2" }),
    );
  });

  it("shows the repository picked, its path cut in the middle, once the field is left", () => {
    renderSearch({ value: "/home/bob/clones/payments-api", repoValid: true });
    expect(screen.getByTestId("target-repo-path")).toHaveTextContent("~/clones/payments-api");
    expect(screen.queryByTestId("repo-search-icon")).toBeNull();
  });

  it("keeps the repository's icon on the repository picked, the field focused, until something is typed", () => {
    renderSearch({ value: "/home/bob/clones/payments-api", repoValid: true });
    const input = screen.getByTestId("target-repo-input");
    fireEvent.focus(input);
    expect(screen.queryByTestId("repo-search-icon")).toBeNull();
    fireEvent.change(input, { target: { value: "risk" } });
    expect(screen.getByTestId("repo-search-icon")).toBeInTheDocument();
  });

  it("gives the repository picked back to the field when Escape drops the search", () => {
    renderSearch({ value: "/home/bob/clones/payments-api", repoValid: true });
    const input = screen.getByTestId("target-repo-input");
    fireEvent.focus(input);
    fireEvent.change(input, { target: { value: "zzz-nothing" } });
    expect(input).toHaveValue("zzz-nothing");
    fireEvent.keyDown(input, { key: "Escape" });
    expect(input).toHaveValue("/home/bob/clones/payments-api");
    expect(screen.queryByTestId("repo-no-match")).toBeNull();
  });
});
