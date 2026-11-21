import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { RunState } from "../../../shared/generated/RunState";
import { pickOption, selectedLabel, selectTrigger } from "../../../test/kitSelect";
import { RUN_MISSING, RUN_UNREADABLE } from "../../model/bugReport";
import { fetchRun } from "../../../run/api/runs";
import { resetToasts, useToasts } from "../../../shared/ui/toast";
import { fetchBugReportDestinations, rememberBugReportDestination, sendBugReport } from "../../api/bugReport";
import type { BugReportDestinations } from "../../../shared/generated/BugReportDestinations";
import BugReportModal, { NOT_SENT } from "./BugReportModal";

const RUN = {
  run_id: "20261120-100000-abcd",
  name: "nightly",
  pipeline_name: "ship",
  status: "failed",
  started_at: "2026-11-20T10:00:00Z",
  completed_at: "2026-11-20T10:30:00Z",
  failure_reason: "node build failed",
  nodes: {
    build: {
      node_id: "build",
      status: "failed",
      iter: 1,
      started_at: "2026-11-20T10:00:00Z",
      completed_at: "2026-11-20T10:04:00Z",
      failure_reason: "exit code 2",
      iterations: [],
      frontmatter_retries: 0,
    },
  },
  node_defs: [{ id: "build", name: "Build", node_type: "agent", view_x: null, view_y: null, inputs: [], outputs: [] }],
} as unknown as RunState;

vi.mock("../../../run/api/runs", () => ({
  fetchRuns: vi.fn(async () => [
    { run_id: "20261120-100000-abcd", pipeline_name: "ship", status: "failed", stalled: false, started_at: null, effective_repo: "" },
    { run_id: "20261120-110000-efgh", pipeline_name: "ship", status: "completed", stalled: false, started_at: null, effective_repo: "" },
  ]),
  fetchRun: vi.fn(async () => RUN),
}));

vi.mock("../../../pipeline/api/pipelines", () => ({
  fetchPipelines: vi.fn(async () => [{ id: "ship", name: "ship", scope: "instance", path: "", node_count: 1, modified: null, variables: {}, prompt_required: false }]),
  fetchPipeline: vi.fn(async () => ({
    id: "ship",
    scope: "instance",
    path: "",
    yaml: "",
    pipeline: {
      name: "ship",
      variables: {},
      nodes: [{ id: "build", name: "Build", type: "agent", inputs: [], outputs: [], interactive: false }],
      edges: [],
    },
    prompts: { build: "SECRET PROMPT" },
    diagnostics: [],
  })),
}));

vi.mock("../../../session/api/sessions", () => ({
  fetchSessions: vi.fn(async () => ({ live: 0, cap: 0, version: "2.81.0" })),
}));

vi.mock("../../api/update", () => ({
  fetchUpdateStatus: vi.fn(async () => {
    throw new Error("offline");
  }),
}));

vi.mock("../../../settings/api/settings", () => ({
  fetchSettings: vi.fn(async () => ({
    host_os: "Linux 6.8.0 (x86_64)",
    harness_descriptors: {
      path: null,
      names: ["claude"],
      rejected: [],
      reason: null,
      harnesses: [{ name: "claude", source: "builtin", installed: true, version: "2.1.322" }],
    },
  })),
}));

vi.mock("../../api/bugReport", () => ({
  fetchBugReportDestinations: vi.fn(async () => ({ destinations: [], last: null })),
  sendBugReport: vi.fn(async () => ({ reference: "BUG-1", link: "/ext/test/bug-reports/1" })),
  rememberBugReportDestination: vi.fn(async () => undefined),
}));

/** What the test extension offers: an inbox that takes the prompts, a board that refuses them. */
const OFFERED: BugReportDestinations = {
  destinations: [
    { id: "test/inbox", label: "Test inbox", accepts_prompts: true, icon: null, extension: "test", extension_name: "Test extension" },
    { id: "test/board", label: "Test board", accepts_prompts: false, icon: null, extension: "test", extension_name: "Test extension" },
  ],
  last: null,
};

const PREFILL = { runId: "20261120-100000-abcd", pipeline: "ship" };

let opened: string[] = [];
beforeEach(() => {
  opened = [];
  vi.mocked(sendBugReport).mockClear();
  vi.mocked(rememberBugReportDestination).mockClear();
  vi.spyOn(window, "open").mockImplementation((url) => {
    opened.push(String(url));
    return null;
  });
});
afterEach(() => {
  vi.restoreAllMocks();
  resetToasts();
});

/** The window, once its menus and details are read. */
async function renderPrefilled(onClose = vi.fn()) {
  render(<BugReportModal prefill={PREFILL} onClose={onClose} />);
  await waitFor(() => expect(selectedLabel(screen.getByTestId("bug-report-run"))).toBe("20261120-100000-abcd · ship"));
  await waitFor(() => expect(selectedLabel(screen.getByTestId("bug-report-pipeline"))).toBe("ship"));
  return onClose;
}

const send = () => screen.getByTestId("bug-report-send");
const describeBug = (text: string) => fireEvent.change(screen.getByTestId("bug-report-description"), { target: { value: text } });
const showPreview = () => fireEvent.click(screen.getByRole("tab", { name: /Preview/ }));
const checkbox = (name: string) => screen.getByRole("checkbox", { name });

describe("BugReportModal", () => {
  it("is prefilled with the selected Run and its Pipeline, both joined by default", async () => {
    await renderPrefilled();
    expect(checkbox("Include run details")).toBeChecked();
    expect(checkbox("Include pipeline details")).toBeChecked();
    expect(screen.getByTestId("bug-report-description")).toHaveAttribute(
      "placeholder",
      "Describe the bug encountered, add a screenshot if possible",
    );
  });

  it("previews the facts, the run's and its nodes' statuses and the failure reason, without a prompt", async () => {
    await renderPrefilled();
    describeBug("The node stays running");
    showPreview();
    const preview = (await screen.findByTestId("bug-report-preview")) as HTMLTextAreaElement;
    await waitFor(() => expect(preview.value).toContain("### Run details"));
    expect(preview.value).toContain("### Description\nThe node stays running");
    expect(preview.value).toContain("PDO 2.81.0");
    expect(preview.value).toContain("Linux 6.8.0 (x86_64)");
    expect(preview.value).toContain("claude 2.1.322");
    expect(preview.value).toContain("Run 20261120-100000-abcd · nightly · failed · 30m 0s");
    expect(preview.value).toContain("Failure: node build failed");
    expect(preview.value).toContain("| Build | agent | failed | 4m 0s | exit code 2 |");
    expect(preview.value).toContain("Pipeline ship · 1 node · 0 edges");
    expect(preview.value).not.toContain("SECRET");
  });

  it("greys Include prompts toward the public tracker", async () => {
    await renderPrefilled();
    expect(checkbox("Include prompts")).toBeDisabled();
    expect(screen.getByText("Not sent to a public tracker")).toBeInTheDocument();
  });

  it("holds the sending while a checked box has no choice, and unchecking it releases it", async () => {
    await renderPrefilled();
    describeBug("x");
    await waitFor(() => expect(send()).not.toHaveAttribute("aria-disabled"));
    fireEvent.click(checkbox("Include run details"));
    expect(send()).not.toHaveAttribute("aria-disabled");
    fireEvent.click(checkbox("Include run details"));
    expect(screen.getByTestId("bug-report-run-error")).toHaveTextContent(RUN_MISSING);
    expect(selectTrigger(screen.getByTestId("bug-report-run"))).toHaveAttribute("aria-invalid", "true");
    expect(send()).toHaveAttribute("aria-disabled", "true");
    fireEvent.click(send());
    expect(opened).toEqual([]);
    fireEvent.click(checkbox("Include run details"));
    expect(screen.queryByTestId("bug-report-run-error")).toBeNull();
    expect(send()).not.toHaveAttribute("aria-disabled");
  });

  it("holds the sending when the checked Run cannot be read", async () => {
    vi.mocked(fetchRun).mockRejectedValueOnce(new Error("gone"));
    render(<BugReportModal prefill={PREFILL} onClose={vi.fn()} />);
    describeBug("x");
    expect(await screen.findByTestId("bug-report-run-error")).toHaveTextContent(RUN_UNREADABLE);
    expect(send()).toHaveAttribute("aria-disabled", "true");
  });

  it("holds the sending of an edited preview whose description was emptied", async () => {
    await renderPrefilled();
    describeBug("x");
    showPreview();
    fireEvent.change(await screen.findByTestId("bug-report-preview"), { target: { value: "### Version\nPDO 2.81.0" } });
    expect(send()).toHaveAttribute("aria-disabled", "true");
  });

  it("holds the sending without a description", async () => {
    await renderPrefilled();
    expect(send()).toHaveAttribute("aria-disabled", "true");
  });

  it("opens a new issue of the public repository on the Bug report template, then closes", async () => {
    const onClose = await renderPrefilled();
    describeBug("The node stays running");
    await waitFor(() => expect(send()).not.toHaveAttribute("aria-disabled"));
    fireEvent.click(send());
    expect(opened).toHaveLength(1);
    const url = new URL(opened[0]);
    expect(`${url.origin}${url.pathname}`).toBe("https://github.com/Loulen/prompt-driven-orchestrator/issues/new");
    expect(url.searchParams.get("template")).toBe("bug_report.yml");
    expect(url.searchParams.get("description")).toBe("The node stays running");
    expect(url.searchParams.get("run")).toContain("exit code 2");
    expect(url.searchParams.get("pipeline")).toContain("Pipeline ship");
    expect(opened[0]).not.toContain("SECRET");
    expect(onClose).toHaveBeenCalled();
  });

  it("sends from either tab with Ctrl+Enter, the edited preview as it is", async () => {
    await renderPrefilled();
    describeBug("first");
    showPreview();
    const preview = (await screen.findByTestId("bug-report-preview")) as HTMLTextAreaElement;
    await waitFor(() => expect(preview.value).toContain("### Run details"));
    fireEvent.change(preview, { target: { value: "### Description\nrewritten by hand\n\n### Version\nPDO 2.81.0" } });
    fireEvent.keyDown(preview, { key: "Enter", ctrlKey: true });
    const url = new URL(opened[0]);
    expect(url.searchParams.get("description")).toBe("rewritten by hand");
    expect(url.searchParams.has("run")).toBe(false);
  });

  it("marks an edited preview, asks before a form change overwrites it, and resets it", async () => {
    await renderPrefilled();
    describeBug("first");
    showPreview();
    const preview = (await screen.findByTestId("bug-report-preview")) as HTMLTextAreaElement;
    fireEvent.change(preview, { target: { value: "my own text" } });
    expect(screen.getByRole("tab", { name: /Preview/ })).toHaveTextContent("edited");

    fireEvent.click(screen.getByRole("tab", { name: "Report" }));
    describeBug("second");
    const ask = screen.getByRole("dialog", { name: "Overwrite the edited preview?" });
    fireEvent.click(within(ask).getByRole("button", { name: "Keep my edits" }));
    expect(screen.getByTestId("bug-report-description")).toHaveValue("first");
    expect(screen.getByRole("tab", { name: /Preview/ })).toHaveTextContent("edited");

    describeBug("second");
    fireEvent.click(within(screen.getByRole("dialog", { name: "Overwrite the edited preview?" })).getByRole("button", { name: "Overwrite" }));
    expect(screen.getByTestId("bug-report-description")).toHaveValue("second");
    expect(screen.getByRole("tab", { name: /Preview/ })).not.toHaveTextContent("edited");

    showPreview();
    fireEvent.change(screen.getByTestId("bug-report-preview"), { target: { value: "again" } });
    fireEvent.click(screen.getByTestId("bug-report-reset"));
    expect((screen.getByTestId("bug-report-preview") as HTMLTextAreaElement).value).toContain("### Description\nsecond");
  });

  it("lets the user pick another Run", async () => {
    await renderPrefilled();
    pickOption(screen.getByTestId("bug-report-run"), "20261120-110000-efgh · ship");
    expect(selectedLabel(screen.getByTestId("bug-report-run"))).toBe("20261120-110000-efgh · ship");
  });

  it("takes a screenshot pasted anywhere, and lets it go", async () => {
    URL.createObjectURL = vi.fn(() => "blob:shot");
    URL.revokeObjectURL = vi.fn();
    await renderPrefilled();
    const image = new File(["png"], "shot.png", { type: "image/png" });
    const paste = new Event("paste", { bubbles: true, cancelable: true }) as Event & { clipboardData: unknown };
    paste.clipboardData = { files: [image], items: [] };
    act(() => {
      window.dispatchEvent(paste);
    });
    expect(paste.defaultPrevented).toBe(true);
    expect(screen.getByTestId("bug-report-capture")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove the screenshot" }));
    expect(screen.queryByTestId("bug-report-capture")).toBeNull();
  });

  it("says where the report goes", async () => {
    await renderPrefilled();
    expect(selectedLabel(screen.getByTestId("bug-report-destination"))).toBe("PDO issues (GitHub)");
    expect(send()).toHaveTextContent("Open GitHub issue");
  });

  describe("Send to", () => {
    const destination = () => screen.getByTestId("bug-report-destination");
    /** The trigger's label, without the letter an extension's icon shows until it loads. */
    const destinationLabel = () => selectTrigger(destination()).getAttribute("title");
    const offer = (last: string | null = null) =>
      vi.mocked(fetchBugReportDestinations).mockResolvedValueOnce({ ...OFFERED, last });
    const toInbox = async () => {
      await waitFor(() => expect(within(destination()).getByRole("button")).toBeInTheDocument());
      pickOption(destination(), "Test inbox");
    };

    it("lists the core's issues first, then each extension's destinations under its name", async () => {
      offer("test/board");
      await renderPrefilled();
      await waitFor(() => expect(destinationLabel()).toBe("Test board"));
      fireEvent.click(selectTrigger(destination()));
      const list = screen.getByRole("listbox", { name: "Send to" });
      const rows = Array.from(list.querySelectorAll(".d-pop__group, [role=option]")).map((el) =>
        el.getAttribute("role") === "option"
          ? [el.querySelector(".d-pop__label")?.textContent, el.querySelector(".d-pop__hint")?.textContent ?? null]
          : el.textContent,
      );
      expect(rows).toEqual([
        "Core",
        ["PDO issues (GitHub)", "public"],
        "Test extension",
        ["Test inbox", null],
        ["Test board", "last used"],
      ]);
      expect(within(list).getByTestId("github-mark")).toBeInTheDocument();
      expect(within(list).getAllByTestId("extension-icon-fallback")).toHaveLength(2);
    });

    it("starts on the core's issues when the last choice is no longer offered", async () => {
      vi.mocked(fetchBugReportDestinations).mockResolvedValueOnce({ ...OFFERED, last: "gone/away" });
      await renderPrefilled();
      expect(selectedLabel(destination())).toBe("PDO issues (GitHub)");
    });

    it("lets Include prompts be checked toward a destination that accepts them, unchecked by default", async () => {
      offer();
      await renderPrefilled();
      await toInbox();
      expect(checkbox("Include prompts")).not.toBeDisabled();
      expect(checkbox("Include prompts")).not.toBeChecked();
      expect(screen.getByText("Node prompts of the run and pipeline")).toBeInTheDocument();
      expect(send()).toHaveTextContent("Send to Test inbox");
      pickOption(destination(), "Test board");
      expect(checkbox("Include prompts")).toBeDisabled();
      expect(screen.getByText("This destination does not accept prompts")).toBeInTheDocument();
    });

    it("keeps the destination of a preview edited before the last choice is read", async () => {
      let answer: (v: BugReportDestinations) => void = () => {};
      vi.mocked(fetchBugReportDestinations).mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
      await renderPrefilled();
      describeBug("x");
      showPreview();
      fireEvent.change(await screen.findByTestId("bug-report-preview"), { target: { value: "### Description\nmine" } });
      await act(async () => answer({ ...OFFERED, last: "test/inbox" }));
      expect(destinationLabel()).toBe("PDO issues (GitHub)");
    });

    it("refuses a screenshot over 10 MB before it leaves", async () => {
      offer();
      URL.createObjectURL = vi.fn(() => "blob:shot");
      URL.revokeObjectURL = vi.fn();
      await renderPrefilled();
      await toInbox();
      describeBug("x");
      const big = new File(["png"], "big.png", { type: "image/png" });
      Object.defineProperty(big, "size", { value: 11 * 1024 * 1024 });
      const paste = new Event("paste", { bubbles: true, cancelable: true }) as Event & { clipboardData: unknown };
      paste.clipboardData = { files: [big], items: [] };
      act(() => {
        window.dispatchEvent(paste);
      });
      await waitFor(() => expect(send()).not.toHaveAttribute("aria-disabled"));
      fireEvent.click(send());
      expect(await screen.findByRole("alert")).toHaveTextContent("The screenshot is over 10 MB");
      expect(sendBugReport).not.toHaveBeenCalled();
    });

    it("sends the report, its screenshot and the prompts, locked while it leaves, then a toast links to the item", async () => {
      offer();
      URL.createObjectURL = vi.fn(() => "blob:shot");
      URL.revokeObjectURL = vi.fn();
      let answer: (v: { reference: string; link: string }) => void = () => {};
      vi.mocked(sendBugReport).mockImplementationOnce(() => new Promise((resolve) => (answer = resolve)));
      const onClose = await renderPrefilled();
      await toInbox();
      describeBug("The node stays running");
      fireEvent.click(checkbox("Include prompts"));
      const paste = new Event("paste", { bubbles: true, cancelable: true }) as Event & { clipboardData: unknown };
      paste.clipboardData = { files: [new File(["png"], "shot.png", { type: "image/png" })], items: [] };
      act(() => {
        window.dispatchEvent(paste);
      });
      await waitFor(() => expect(send()).not.toHaveAttribute("aria-disabled"));
      fireEvent.click(send());

      await waitFor(() => expect(sendBugReport).toHaveBeenCalled());
      const body = vi.mocked(sendBugReport).mock.calls[0][0];
      expect(body.destination).toBe("test/inbox");
      expect(body.fields.description).toBe("The node stays running");
      expect(body.fields.prompts).toContain("SECRET PROMPT");
      expect(body.text).toContain("### Prompts");
      expect(body.screenshot).toEqual({ media_type: "image/png", data: "cG5n" });
      expect(opened).toEqual([]);

      // Locked: Sending…, no Cancel, no close, no Escape.
      expect(send()).toHaveTextContent("Sending…");
      expect(send()).toHaveAttribute("data-pending");
      expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
      expect(screen.queryByRole("button", { name: "Close" })).toBeNull();
      fireEvent.keyDown(screen.getByTestId("bug-report"), { key: "Escape" });
      expect(onClose).not.toHaveBeenCalled();

      await act(async () => answer({ reference: "BUG-1", link: "/ext/test/bug-reports/1" }));
      expect(onClose).toHaveBeenCalled();
      const [toast] = useToasts.getState().toasts;
      expect(toast.message).toBe("Bug report sent: BUG-1");
      expect(toast.kind).toBe("success");
      toast.action?.run();
      expect(opened).toEqual(["/ext/test/bug-reports/1"]);
    });

    it("keeps the window open with the destination's refusal in a Banner", async () => {
      offer();
      vi.mocked(sendBugReport).mockRejectedValueOnce(new Error("the inbox is full"));
      const onClose = await renderPrefilled();
      await toInbox();
      describeBug("x");
      await waitFor(() => expect(send()).not.toHaveAttribute("aria-disabled"));
      fireEvent.click(send());
      const banner = await screen.findByRole("alert");
      expect(banner).toHaveTextContent(NOT_SENT);
      expect(banner).toHaveTextContent("the inbox is full");
      expect(onClose).not.toHaveBeenCalled();
      expect(send()).toHaveTextContent("Send to Test inbox");
    });

    it("remembers the core's issues as the last choice once the browser opened them", async () => {
      offer("test/inbox");
      await renderPrefilled();
      await waitFor(() => expect(destinationLabel()).toBe("Test inbox"));
      pickOption(destination(), "PDO issues (GitHub)");
      describeBug("x");
      await waitFor(() => expect(send()).not.toHaveAttribute("aria-disabled"));
      fireEvent.click(send());
      expect(opened).toHaveLength(1);
      expect(rememberBugReportDestination).toHaveBeenCalledWith("github");
      expect(sendBugReport).not.toHaveBeenCalled();
    });
  });
});
