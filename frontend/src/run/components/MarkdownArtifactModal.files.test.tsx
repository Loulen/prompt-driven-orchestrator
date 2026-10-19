import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const fetchArtifactMock = vi.fn();

vi.mock("../api/artifacts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../api/artifacts")>()),
  fetchArtifact: (...args: unknown[]) => fetchArtifactMock(...args),
}));
vi.mock("../api/nodes", () => ({ fetchNodeIO: vi.fn() }));
vi.mock("react-markdown", () => ({
  default: ({ children }: { children: string }) => <div>{children}</div>,
}));

import MarkdownArtifactModal from "./MarkdownArtifactModal";
import { filesPreviewKind } from "../model/nodeDetailFormat";
import type { FileInfo } from "../api/nodes";

function file(path: string): FileInfo {
  return { path, exists: true, size: 10, frontmatter: null };
}

function openFiles(files: FileInfo[]) {
  render(
    <MarkdownArtifactModal
      runId="run-1"
      portName="exports"
      portType="files"
      source={{ kind: "static", files }}
      onClose={() => {}}
    />,
  );
}

describe("MarkdownArtifactModal — files port and downloads", () => {
  beforeEach(() => {
    fetchArtifactMock.mockReset();
    fetchArtifactMock.mockResolvedValue("a,b\n1,2");
  });

  it("previews each file by its format", () => {
    expect(filesPreviewKind("x/cart.PNG")).toBe("image");
    expect(filesPreviewKind("x/rows.csv")).toBe("text");
    expect(filesPreviewKind("x/notes.md")).toBe("text");
    expect(filesPreviewKind("x/report.pdf")).toBe("none");
    expect(filesPreviewKind(undefined)).toBe("none");
  });

  it("a binary file is never fetched as text: it offers its download", () => {
    openFiles([file("w/iter-1/exports/report.pdf"), file("w/iter-1/exports/rows.csv")]);
    expect(fetchArtifactMock).not.toHaveBeenCalled();
    expect(screen.getByTestId("files-no-preview")).toHaveTextContent("No preview for this format.");
    expect(screen.getByTestId("artifact-download").getAttribute("href")).toBe(
      "/runs/run-1/artifact?path=w%2Fiter-1%2Fexports%2Freport.pdf&download=true",
    );
    expect(screen.queryByTestId("artifact-copy")).not.toBeInTheDocument();
  });

  it("a text file of the port shows as text, and the download follows the file shown", async () => {
    openFiles([file("w/iter-1/exports/report.pdf"), file("w/iter-1/exports/rows.csv")]);
    fireEvent.click(screen.getByLabelText("Next file"));
    await waitFor(() => expect(screen.getByTestId("files-text")).toHaveTextContent("a,b 1,2"));
    expect(fetchArtifactMock).toHaveBeenCalledWith("run-1", "w/iter-1/exports/rows.csv");
    expect(screen.getByTestId("artifact-download").getAttribute("href")).toBe(
      "/runs/run-1/artifact?path=w%2Fiter-1%2Fexports%2Frows.csv&download=true",
    );
  });

  it("paging to another text file never shows the previous one's text", async () => {
    let resolveB: (text: string) => void = () => {};
    fetchArtifactMock
      .mockResolvedValueOnce("first")
      .mockImplementationOnce(() => new Promise<string>((r) => (resolveB = r)));
    openFiles([file("w/iter-1/exports/a.csv"), file("w/iter-1/exports/b.csv")]);
    await waitFor(() => expect(screen.getByTestId("files-text")).toHaveTextContent("first"));
    fireEvent.click(screen.getByLabelText("Next file"));
    expect(screen.queryByTestId("files-text")).not.toBeInTheDocument();
    expect(screen.getByText("Loading...")).toBeInTheDocument();
    resolveB("second");
    await waitFor(() => expect(screen.getByTestId("files-text")).toHaveTextContent("second"));
  });

  it("a markdown artifact carries a download in its header", async () => {
    fetchArtifactMock.mockResolvedValue("# Plan");
    render(
      <MarkdownArtifactModal
        runId="run-1"
        portName="plan"
        source={{ kind: "static", files: [file("planner/iter-1/plan/output.md")] }}
        onClose={() => {}}
      />,
    );
    await waitFor(() => expect(screen.getByText("# Plan")).toBeInTheDocument());
    const link = screen.getByTestId("artifact-download");
    expect(link).toHaveAttribute("download");
    expect(link.getAttribute("href")).toBe(
      "/runs/run-1/artifact?path=planner%2Fiter-1%2Fplan%2Foutput.md&download=true",
    );
  });

  it("an inline document has no download", () => {
    render(
      <MarkdownArtifactModal
        runId=""
        portName="What's new"
        source={{ kind: "inline", content: "# 2.0" }}
        onClose={() => {}}
      />,
    );
    expect(screen.queryByTestId("artifact-download")).not.toBeInTheDocument();
  });
});
