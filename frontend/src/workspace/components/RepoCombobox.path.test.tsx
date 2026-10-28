import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import RepoCombobox from "./RepoCombobox";

// CORE-108: the chosen path is cut in its middle (the last segment stays whole,
// the full path on hover) and the field's mark sits beside it, never over it.
// jsdom computes no layout: the overlap itself is pinned by the e2e
// `new-run-target-repo.spec.ts`; here, the structure that rules it out.

vi.mock("../../settings/api/settings", () => ({}));
vi.mock("../../settings/api/agentProfiles", async () => await import("../../settings/api/settings"));

const HOME = "/home/user";
const DEEP = `${HOME}/work/clients/very-long-organisation-name/platform/frontend-application`;

function renderCombobox(overrides: Partial<Parameters<typeof RepoCombobox>[0]> = {}) {
  render(
    <RepoCombobox
      value={DEEP}
      onChange={vi.fn()}
      recentRepos={[]}
      repoValid={true}
      repoValidating={false}
      repoError={null}
      home={HOME}
      {...overrides}
    />,
  );
}

describe("RepoCombobox — the chosen path (CORE-108)", () => {
  it("shows the path cut in its middle, the last segment whole", () => {
    renderCombobox();
    expect(screen.getByTestId("target-repo-path-head")).toHaveTextContent(
      "~/work/clients/very-long-organisation-name/platform",
    );
    expect(screen.getByTestId("target-repo-path-tail")).toHaveTextContent("/frontend-application");
    // The input's own text is hidden under it, still the value.
    expect(screen.getByTestId("target-repo-input")).toHaveValue(DEEP);
    expect(screen.getByTestId("target-repo-input")).toHaveClass("text-transparent");
  });

  it("gives the full path on hover", () => {
    renderCombobox();
    expect(screen.getByTestId("target-repo-input")).toHaveAttribute("title", DEEP);
  });

  it("edits the raw value once focused", () => {
    renderCombobox();
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.queryByTestId("target-repo-path")).toBeNull();
    expect(screen.getByTestId("target-repo-input")).not.toHaveClass("text-transparent");
    fireEvent.blur(screen.getByTestId("target-repo-input"));
    expect(screen.getByTestId("target-repo-path")).toBeInTheDocument();
  });

  it("shows nothing over an empty field", () => {
    renderCombobox({ value: "" });
    expect(screen.queryByTestId("target-repo-path")).toBeNull();
  });

  it("keeps a folder with no parent whole", () => {
    renderCombobox({ value: "repo" });
    expect(screen.queryByTestId("target-repo-path-head")).toBeNull();
    expect(screen.getByTestId("target-repo-path-tail")).toHaveTextContent("repo");
  });

  it("shows the root as typed", () => {
    renderCombobox({ value: "/" });
    expect(screen.getByTestId("target-repo-path-tail")).toHaveTextContent("/");
  });

  it("lays the mark and the loupe beside the path, not over it", () => {
    renderCombobox({ badge: <span data-testid="the-badge">default</span> });
    const badge = screen.getByTestId("the-badge");
    const loupe = screen.getByTestId("repo-browse-trigger");
    // Siblings of the text box in one row: none is laid over it.
    expect(badge.closest(".absolute")).toBeNull();
    expect(loupe).not.toHaveClass("absolute");
    expect(screen.getByTestId("target-repo-path").parentElement).not.toContainElement(badge);
  });
});

describe("RepoCombobox — the lists and the field (CORE-116)", () => {
  it("cuts each listed path in its middle: the folder's own name stays whole", () => {
    renderCombobox({ value: "", recentRepos: [DEEP, `${HOME}/work/infra`] });
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    const [deep] = screen.getAllByTestId("recent-repo-item");
    expect(deep.querySelector(".ln-path__tail")).toHaveTextContent("/frontend-application");
    expect(deep.querySelector(".ln-path__head")).toHaveTextContent("~/work/clients/very-long-organisation-name/platform");
    expect(deep).toHaveAttribute("title", DEEP);
  });

  it("cuts a grouped path in its middle too", () => {
    renderCombobox({ value: "", groups: [{ label: "Platform", repos: [DEEP] }] });
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.getByTestId("repo-group-item").querySelector(".ln-path__tail")).toHaveTextContent(
      "/frontend-application",
    );
  });

  it("does not spell-check a path", () => {
    renderCombobox();
    expect(screen.getByTestId("target-repo-input")).toHaveAttribute("spellcheck", "false");
  });

  it("rings the field on the input's focus only, not on the loupe's", () => {
    renderCombobox();
    // The ring is `:has(.ln-field__input:focus)` on the field, which holds the
    // input and not the loupe (a sibling of the field).
    const field = screen.getByTestId("target-repo-input").closest(".ln-field")!;
    expect(field).not.toContainElement(screen.getByTestId("repo-browse-trigger"));
  });

  it("edges the field in the failed tone on an invalid repository", () => {
    renderCombobox({ repoValid: false, repoError: "Not a git repository" });
    expect(screen.getByTestId("target-repo-input").closest(".ln-field")).toHaveAttribute("data-invalid");
    expect(screen.getByTestId("repo-error")).toHaveTextContent("Not a git repository");
  });
});

describe("RepoCombobox — when its list opens (CORE-116)", () => {
  it("does not open on focus when the field holds a listed repository; the chevron opens it", () => {
    renderCombobox({ groups: [{ label: "Platform", repos: [DEEP, `${HOME}/work/infra`] }] });
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.queryByTestId("repo-groups-dropdown")).toBeNull();
    fireEvent.click(screen.getByTestId("repo-groups-trigger"));
    expect(screen.getByTestId("repo-groups-dropdown")).toBeInTheDocument();
  });

  it("steps aside once a listed path is typed whole", () => {
    const onChange = vi.fn();
    renderCombobox({ value: "", onChange, recentRepos: [DEEP] });
    fireEvent.focus(screen.getByTestId("target-repo-input"));
    expect(screen.getByTestId("recent-repos-dropdown")).toBeInTheDocument();
    fireEvent.change(screen.getByTestId("target-repo-input"), { target: { value: DEEP } });
    expect(onChange).toHaveBeenCalledWith(DEEP);
    expect(screen.queryByTestId("recent-repos-dropdown")).toBeNull();
  });
});
