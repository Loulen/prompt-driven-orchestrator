import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";

const { importMock, sendMock, settingsMock, clipboardMock } = vi.hoisted(() => ({
  importMock: vi.fn(),
  sendMock: vi.fn(),
  settingsMock: vi.fn(),
  clipboardMock: vi.fn(),
}));

vi.mock("../api", async () => {
  const actual = await vi.importActual<typeof import("../api")>("../api");
  return {
    ...actual,
    importNodeFiles: (...args: unknown[]) => importMock(...args),
    sendTextToNodeTerminal: (...args: unknown[]) => sendMock(...args),
    fetchSettings: () => settingsMock(),
  };
});

vi.mock("../lib/terminalClipboard", () => ({
  writeClipboardText: (text: string) => clipboardMock(text),
}));

import { ApiError } from "../api";
import ImportFilesModal from "./ImportFilesModal";

const PATH = ".pdo/artifacts/_attachments/grill/contrat.pdf";
const TEXT = `I imported a file for you: \`${PATH}\` (path relative to the worktree root).`;

function pdf(name = "contrat.pdf", bytes = 4) {
  return new File(["x".repeat(bytes)], name, { type: "application/pdf" });
}

function setup(props: Partial<React.ComponentProps<typeof ImportFilesModal>> = {}) {
  const onClose = vi.fn();
  render(
    <ImportFilesModal
      runId="run-1"
      nodeId="grill"
      iter={2}
      initialFiles={[pdf()]}
      onClose={onClose}
      {...props}
    />,
  );
  return { onClose };
}

describe("ImportFilesModal (#971)", () => {
  beforeEach(() => {
    importMock.mockReset();
    sendMock.mockReset();
    clipboardMock.mockReset().mockResolvedValue(true);
    settingsMock.mockReset().mockResolvedValue({
      max_attachments_mb: { effective: 1, source: "stored", stored: 1, env: null, default: 50 },
    });
  });

  it("opens with the dropped PDF pre-filled, imports it, then shows the text with its final path", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    setup();
    expect(screen.getByTestId("import-files")).toHaveTextContent("contrat.pdf");
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    expect(importMock).toHaveBeenCalledWith("run-1", "grill", 2, [expect.any(File)]);
    expect(screen.getByTestId("import-text")).toHaveTextContent(TEXT);
  });

  it("several files in one gesture, and a suffixed name is the one shown", async () => {
    const suffixed = ".pdo/artifacts/_attachments/grill/contrat-1.pdf";
    importMock.mockResolvedValue({
      iter: 2,
      files: [
        { name: "contrat-1.pdf", path: suffixed, size: 4 },
        { name: "annexe.csv", path: ".pdo/artifacts/_attachments/grill/annexe.csv", size: 2 },
      ],
    });
    setup({ initialFiles: [pdf(), new File(["ab"], "annexe.csv")] });
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 2 files");
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    expect(importMock.mock.calls[0][3]).toHaveLength(2);
    expect(screen.getByTestId("import-text")).toHaveTextContent(`\`${suffixed}\``);
  });

  it("the copy icon copies the text", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    fireEvent.click(screen.getByTestId("import-copy"));
    await waitFor(() => expect(clipboardMock).toHaveBeenCalledWith(TEXT));
    expect(sendMock).not.toHaveBeenCalled();
  });

  it("Copy & send to terminal copies AND writes the text into the terminal", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    sendMock.mockResolvedValue({ ok: true });
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    fireEvent.click(screen.getByTestId("import-send"));
    await screen.findByTestId("import-sent");
    expect(clipboardMock).toHaveBeenCalledWith(TEXT);
    expect(sendMock).toHaveBeenCalledWith("run-1", "grill", 2, TEXT);
    expect(screen.getByTestId("import-sent")).toHaveTextContent("not submitted");
  });

  it("a failed terminal write says the text is in the clipboard", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    sendMock.mockRejectedValue(new ApiError("node grill has no live session; nothing written", { status: 409 }));
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    fireEvent.click(screen.getByTestId("import-send"));
    const failed = await screen.findByTestId("import-send-failed");
    expect(failed).toHaveTextContent("no live session");
    expect(failed).toHaveTextContent("in your clipboard");
  });

  it("over the budget: names the setting and Import stays off", async () => {
    setup({ initialFiles: [pdf("big.pdf", 2 * 1024 * 1024)] });
    const warning = await screen.findByTestId("import-over-budget");
    expect(warning).toHaveTextContent("max_attachments_mb");
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    expect(importMock).not.toHaveBeenCalled();
  });

  it("a daemon refusal is shown in place and the modal stays on the selection", async () => {
    importMock.mockRejectedValue(
      new ApiError(
        "`contrat.pdf` takes the import past the limit of 1 MB per import; raise `max_attachments_mb` in Settings or import less. nothing imported",
        { status: 413 },
      ),
    );
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    const error = await screen.findByTestId("import-error");
    expect(error).toHaveTextContent("max_attachments_mb");
    expect(screen.queryByTestId("import-done")).toBeNull();
  });

  it("a node without a live session: the reason is shown and nothing can be imported", () => {
    setup({ blockedReason: "This node has no live session" });
    expect(screen.getByTestId("import-blocked")).toHaveTextContent("no live session");
    expect(screen.getByTestId("import-submit")).toBeDisabled();
  });

  it("files dropped on the modal are added to the list", () => {
    setup({ initialFiles: [] });
    fireEvent.drop(screen.getByTestId("import-files-modal"), {
      dataTransfer: { files: [pdf("b.pdf")], items: [{ kind: "file" }], types: ["Files"] },
    });
    expect(screen.getByTestId("import-files")).toHaveTextContent("b.pdf");
  });

  it("Escape closes", () => {
    const { onClose } = setup();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
