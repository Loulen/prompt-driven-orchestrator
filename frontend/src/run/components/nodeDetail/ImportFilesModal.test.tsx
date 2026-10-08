import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { ApiError } from "../../../shared/http/client";

const { importMock, sendMock, settingsMock, clipboardMock } = vi.hoisted(() => ({
  importMock: vi.fn(),
  sendMock: vi.fn(),
  settingsMock: vi.fn(),
  clipboardMock: vi.fn(),
}));

vi.mock("../../api/nodes", () => ({
  importNodeFiles: (...args: unknown[]) => importMock(...args),
}));
vi.mock("../../../session/api/sessions", () => ({
  sendTextToNodeTerminal: (...args: unknown[]) => sendMock(...args),
}));
vi.mock("../../../settings/api/settings", async () => ({
  ...(await vi.importActual<typeof import("../../../settings/api/settings")>(
    "../../../settings/api/settings",
  )),
  fetchSettings: () => settingsMock(),
}));
vi.mock("../../../session/model/terminalClipboard", () => ({
  writeClipboardText: (text: string) => clipboardMock(text),
}));

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

describe("ImportFilesModal", () => {
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
    expect(screen.getByText("Never committed, removed with the Run.")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    expect(importMock).toHaveBeenCalledWith("run-1", "grill", 2, [expect.any(File)]);
    expect(screen.getByTestId("import-text")).toHaveTextContent(TEXT);
  });

  it("several files in one gesture, and a suffixed name is the one shown", async () => {
    const suffixed = ".pdo/artifacts/_attachments/grill/contrat-1.pdf";
    const annexe = ".pdo/artifacts/_attachments/grill/annexe.csv";
    importMock.mockResolvedValue({
      iter: 2,
      files: [
        { name: "contrat-1.pdf", path: suffixed, size: 4 },
        { name: "annexe.csv", path: annexe, size: 2 },
      ],
    });
    setup({ initialFiles: [pdf(), new File(["a,"], "annexe.csv")] });
    expect(screen.getByTestId("import-submit")).toHaveTextContent("Import 2 files");
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    expect(screen.getByTestId("import-done")).toHaveTextContent("2 files imported.");
    const phrases = screen.getAllByTestId("import-phrase").map((p) => p.textContent);
    expect(phrases).toEqual([
      `I imported a file for you: \`${suffixed}\` (path relative to the worktree root).`,
      `I imported a file for you: \`${annexe}\` (path relative to the worktree root).`,
    ]);
  });

  it("over the instance budget: a clear message and Import stays off", async () => {
    setup({ initialFiles: [pdf("big.pdf", 2 * 1024 * 1024)] });
    await screen.findByTestId("import-over-budget");
    expect(screen.getByTestId("import-over-budget")).toHaveTextContent("max_attachments_mb");
    expect(screen.getByTestId("import-submit")).toBeDisabled();
    expect(importMock).not.toHaveBeenCalled();
  });

  it("a refusal of the daemon is shown in place, nothing imported", async () => {
    importMock.mockRejectedValue(
      new ApiError("node grill iter 2 has no live session; nothing imported", { status: 409 }),
    );
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    expect(await screen.findByTestId("import-error")).toHaveTextContent("nothing imported");
    expect(screen.queryByTestId("import-done")).toBeNull();
  });

  it("a blocked node says why and cannot import", () => {
    setup({ blockedReason: "This node has no live session." });
    expect(screen.getByTestId("import-blocked")).toHaveTextContent("no live session");
    expect(screen.getByTestId("import-submit")).toBeDisabled();
  });

  it("Copy copies the text", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    fireEvent.click(screen.getByTestId("import-copy"));
    await waitFor(() => expect(clipboardMock).toHaveBeenCalledWith(TEXT));
    expect(sendMock).not.toHaveBeenCalled();
  });

  it("Copy & send to terminal copies, writes the text without submitting it, and closes without confirmation", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    sendMock.mockResolvedValue({ ok: true, iter: 2 });
    const { onClose } = setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    const send = screen.getByTestId("import-send");
    expect(send).toHaveAttribute("title", expect.stringContaining("without submitting"));
    fireEvent.click(send);
    await waitFor(() => expect(onClose).toHaveBeenCalledTimes(1));
    expect(clipboardMock).toHaveBeenCalledWith(TEXT);
    expect(sendMock).toHaveBeenCalledWith("run-1", "grill", 2, TEXT);
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("a failed terminal write keeps the modal open and says the text is in the clipboard", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    sendMock.mockRejectedValue(new ApiError("tmux refused the paste", { status: 502 }));
    const { onClose } = setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    fireEvent.click(screen.getByTestId("import-send"));
    const failed = await screen.findByTestId("import-send-failed");
    expect(failed).toHaveTextContent("tmux refused the paste");
    expect(failed).toHaveTextContent("in your clipboard");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("the path wraps after a slash, never in the middle of a name", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    setup();
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    const phrase = screen.getByTestId("import-phrase");
    expect(phrase.querySelectorAll("wbr")).toHaveLength(PATH.split("/").length - 1);
    expect(screen.getByTestId("import-text")).not.toHaveClass("break-all");
  });

  it("a browser that only watches the terminal can copy the text, not send it", async () => {
    importMock.mockResolvedValue({ iter: 2, files: [{ name: "contrat.pdf", path: PATH, size: 4 }] });
    const { onClose } = setup({ sendBlockedReason: "Another browser has control of the terminal." });
    fireEvent.click(screen.getByTestId("import-submit"));
    await screen.findByTestId("import-done");
    const send = screen.getByTestId("import-send");
    expect(send).toBeDisabled();
    expect(screen.getByTestId("import-send-blocked")).toHaveTextContent("Another browser has control");
    fireEvent.click(send);
    fireEvent.click(screen.getByTestId("import-copy"));
    await waitFor(() => expect(clipboardMock).toHaveBeenCalledWith(TEXT));
    expect(sendMock).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("Escape closes the modal", () => {
    const { onClose } = setup();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
  });
});
