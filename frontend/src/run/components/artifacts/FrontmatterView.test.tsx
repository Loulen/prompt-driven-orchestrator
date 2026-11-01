/**
 * The frontmatter as the Run shows it, in the viewer's band and the I/O rows:
 * a value copies on click, a web address opens, a cut value unfolds.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import FrontmatterView from "./FrontmatterView";

const PR = "https://github.com/acme/app/compare/main...feature/a-very-long-branch-name";

function setClipboard(clipboard: unknown) {
  Object.defineProperty(navigator, "clipboard", { value: clipboard, configurable: true, writable: true });
}

function value(field: string) {
  return screen.getAllByTestId("frontmatter-value").find((v) => v.dataset.field === field)!;
}

beforeEach(() => {
  Object.defineProperty(document, "execCommand", { value: undefined, configurable: true, writable: true });
});
afterEach(() => {
  setClipboard(undefined);
  vi.restoreAllMocks();
});

describe.each(["band", "rows"] as const)("FrontmatterView (%s)", (layout) => {
  it("copies the exact value clicked and says so", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    setClipboard({ writeText });
    render(<FrontmatterView values={{ verdict: "pass", PR_link: PR, count: 3 }} layout={layout} />);
    await act(async () => {
      fireEvent.click(value("PR_link"));
    });
    expect(writeText).toHaveBeenCalledWith(PR);
    expect(screen.getByTestId("frontmatter-copied")).toHaveTextContent("✓ Copied");
    await act(async () => {
      fireEvent.click(value("count"));
    });
    expect(writeText).toHaveBeenLastCalledWith("3");
  });

  it("says when the copy failed", async () => {
    setClipboard({ writeText: vi.fn().mockRejectedValue(new Error("denied")) });
    render(<FrontmatterView values={{ verdict: "pass" }} layout={layout} />);
    await act(async () => {
      fireEvent.click(value("verdict"));
    });
    expect(screen.getByTestId("frontmatter-copy-failed")).toHaveTextContent("Copy failed");
    expect(screen.queryByTestId("frontmatter-copied")).toBeNull();
  });

  it("offers ↗ for an http(s) value only, opening it in a new tab", () => {
    render(
      <FrontmatterView
        values={{ PR_link: PR, app: "http://192.168.1.143:6391/", ftp: "ftp://host/file", note: "see https://x.y" }}
        layout={layout}
      />,
    );
    const links = screen.getAllByTestId("frontmatter-open");
    expect(links.map((a) => a.getAttribute("href"))).toEqual([PR, "http://192.168.1.143:6391/"]);
    expect(links[0]).toHaveAttribute("target", "_blank");
    expect(links[0]).toHaveAttribute("rel", "noopener noreferrer");
    expect(screen.getByRole("link", { name: "Open PR_link" })).toBe(links[0]);
  });

  it("keeps a verdict's tone", () => {
    render(<FrontmatterView values={{ verdict: "PASS", status: "needs_work", note: "hello" }} layout={layout} />);
    expect(value("verdict")).toHaveAttribute("data-st", "done");
    expect(value("status")).toHaveAttribute("data-st", "awaiting");
    expect(value("note")).not.toHaveAttribute("data-st");
  });

  it("shows no chevron when every value fits", () => {
    render(<FrontmatterView values={{ verdict: "pass" }} layout={layout} />);
    expect(screen.queryByTestId("frontmatter-toggle")).toBeNull();
  });

  it("unfolds a cut value, then folds it back", () => {
    // Every value wider than its box: cut.
    vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockReturnValue(400);
    vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockReturnValue(120);
    render(<FrontmatterView values={{ PR_link: PR }} layout={layout} testId="fm" />);
    const toggle = screen.getByTestId("frontmatter-toggle");
    expect(toggle).toHaveAccessibleName("Unfold the frontmatter");
    expect(screen.getByTestId("fm")).not.toHaveAttribute("data-expanded");
    fireEvent.click(toggle);
    expect(screen.getByTestId("fm")).toHaveAttribute("data-expanded");
    expect(screen.getByTestId("frontmatter-toggle")).toHaveAccessibleName("Fold the frontmatter");
    expect(value("PR_link")).toHaveTextContent(PR);
    fireEvent.click(screen.getByTestId("frontmatter-toggle"));
    expect(screen.getByTestId("fm")).not.toHaveAttribute("data-expanded");
  });
});

describe("FrontmatterView with a schema check", () => {
  it("paints a field off schema with its rule, and names a declared field absent", () => {
    render(
      <FrontmatterView
        values={{ status: "drafting", step: "three of five" }}
        layout="rows"
        warnings={[
          { field: "step", reason: "expected int, got string" },
          { field: "owner", reason: "missing required field" },
        ]}
      />,
    );
    expect(screen.getAllByTestId("frontmatter-field")).toHaveLength(1);
    const warnings = screen.getAllByTestId("frontmatter-field-warning");
    expect(warnings.map((w) => w.textContent)).toEqual(["stepthree of five", "ownermissing"]);
    expect(value("step")).toHaveAttribute("data-st", "awaiting");
  });
});
