import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useEffect } from "react";
import Markdown from "react-markdown";
import TopBar from "../../app/TopBar";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { RunListEntry } from "../generated/RunListEntry";
import type { RunState } from "../generated/RunState";
import type { UiCommand } from "../generated/UiCommand";
import type { UiContribution } from "../generated/UiContribution";
import { registerTransientOverlay } from "../../lib/overlays";
import { MARKDOWN_COMPONENTS, REMARK_PLUGINS } from "../markdown/plugins";
import { resetToasts } from "../ui/toast";
import Toaster from "../ui/Toaster";
import { TooltipProvider } from "../ui/tooltip";
import type { Project } from "../../workspace/model/project";
import { projectOfRun } from "./context";
import ExtensionContextProvider from "./ExtensionContextProvider";
import type { CommandContext, CommandHost, PageProps } from "./contract";
import { resetPages } from "./deepLink";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, type BundleModule } from "./store";

function extension(
  id: string,
  contributions: UiContribution[],
  commands: UiCommand[] = [],
): ExtensionEntry {
  return {
    id,
    name: `Extension ${id}`,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands,
      locations: [],
    },
  };
}

function page(id: string, label: string, component: string, icon?: string): UiContribution {
  return { location: "topbar/end", id, label, type: "page", component, ...(icon ? { icon } : {}) };
}

const calls: { context: CommandContext; host: CommandHost }[] = [];

/** One bundle for every extension of these tests. */
const bundle: BundleModule = {
  ContextPage: ({ context, navigate, shell }: PageProps) => {
    useEffect(() => shell.setBreadcrumb(context.path ? context.path.split("/") : []), [shell, context.path]);
    return (
      <div data-testid="context-page">
        <p>run {context.run?.id ?? "none"}</p>
        <p>project {context.project?.name ?? "none"}</p>
        <p>path {context.path || "(root)"}</p>
        <button type="button" onClick={() => navigate("a/b")}>
          go deeper
        </button>
      </div>
    );
  },
  OtherPage: () => <p>the other page</p>,
  record: (context: CommandContext, host: CommandHost) => {
    calls.push({ context, host });
    host.notify(`called on ${context.run?.id}`);
  },
  explode: () => {
    throw new Error("it broke");
  },
};

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify(extensions), {
            status: 200,
            headers: { "Content-Type": "application/json" },
          })
        : new Response("not found", { status: 404 }),
    ),
  );
}

const runs = [
  { run_id: "r1", pipeline_name: "demo", status: "running", effective_repo: "/repos/pdo" },
  { run_id: "r2", pipeline_name: "demo", status: "done", effective_repo: "/repos/other" },
  { run_id: "r1-child", pipeline_name: "demo", status: "running", effective_repo: "/elsewhere", parent_run_id: "r1" },
] as unknown as RunListEntry[];

const projects = [
  { id: "p1", name: "PDO", members: ["/repos/pdo"], attributes: { "test/team": "core" } },
] as unknown as Project[];

function runState(id: string, status = "running"): RunState {
  return { run_id: id, pipeline_name: "demo", status, name: `Run ${id}` } as unknown as RunState;
}

function renderShell(selected: RunState | null = runState("r1")) {
  const ui = (run: RunState | null) => (
    <TooltipProvider>
      <ExtensionContextProvider selectedRun={run} runs={runs} projects={projects}>
        <TopBar onOpenSettings={() => {}} onOpenStats={() => {}} />
        <Toaster />
      </ExtensionContextProvider>
    </TooltipProvider>
  );
  const view = render(ui(selected));
  return { ...view, select: (run: RunState | null) => view.rerender(ui(run)) };
}

beforeEach(() => {
  resetExtensions();
  resetPages();
  resetToasts();
  calls.length = 0;
  window.history.replaceState(null, "", "/");
  setBundleLoader(async () => bundle);
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
  resetPages();
});

describe("the topbar/end location", () => {
  it("shows its icons before Stats, and a page opens full-window without rail with the context", async () => {
    serve([extension("demo", [page("home", "Demo", "ContextPage")])]);
    const { select } = renderShell();
    const button = await screen.findByTestId("extension-topbar-end-demo:home");
    const group = button.closest("div.ml-auto") as HTMLElement;
    const buttons = within(group).getAllByRole("button");
    expect(buttons.indexOf(button)).toBeLessThan(buttons.indexOf(screen.getByTestId("open-stats")));
    // No icon given: the label's first letter, boxed.
    expect(within(button).getByTestId("extension-icon-fallback")).toHaveTextContent("D");
    expect(button).toHaveAccessibleName("Demo");

    fireEvent.click(button);
    const shell = await screen.findByTestId("extension-page");
    expect(within(shell).getByText("run r1")).toBeInTheDocument();
    expect(within(shell).getByText("project PDO")).toBeInTheDocument();
    expect(within(shell).queryByRole("tablist")).toBeNull();
    expect(window.location.hash).toBe("#/ext/demo/topbar/end");

    // Another selection: the context follows.
    select(runState("r2"));
    expect(within(shell).getByText("run r2")).toBeInTheDocument();
    expect(within(shell).getByText("project none")).toBeInTheDocument();

    // The page moves deeper: its deep link and the shell's breadcrumb follow.
    fireEvent.click(within(shell).getByText("go deeper"));
    expect(within(shell).getByText("path a/b")).toBeInTheDocument();
    expect(window.location.hash).toBe("#/ext/demo/topbar/end/a/b");
    expect(screen.getByTestId("extension-page-breadcrumb")).toHaveTextContent("ab");

    // Escape closes it and leaves its deep link.
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("extension-page")).toBeNull();
    expect(window.location.hash).toBe("");
  });

  it("draws a served SVG icon once it loads", async () => {
    class LoadingImage {
      onload: (() => void) | null = null;
      onerror: (() => void) | null = null;
      set src(_: string) {
        setTimeout(() => this.onload?.(), 0);
      }
    }
    vi.stubGlobal("Image", LoadingImage);
    serve([extension("iconic", [page("home", "Iconic", "ContextPage", "/ext/iconic/icon.svg")])]);
    renderShell();
    const button = await screen.findByTestId("extension-topbar-end-iconic:home");
    const icon = await within(button).findByTestId("extension-icon");
    expect(icon.getAttribute("style")).toContain("/ext/iconic/icon.svg");
  });

  it("lists every extension's contributions, in registration order", async () => {
    serve([
      extension("first", [page("a", "Alpha", "OtherPage")]),
      extension("second", [page("b", "Beta", "OtherPage"), page("c", "Gamma", "OtherPage")]),
    ]);
    renderShell();
    const group = await screen.findByTestId("extension-topbar-end");
    await waitFor(() => expect(within(group).getAllByRole("button")).toHaveLength(3));
    expect(within(group).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
      "Alpha",
      "Beta",
      "Gamma",
    ]);
  });
});

describe("actions and commands", () => {
  const record = { id: "demo.record", title: "Record", export: "record" };
  const explode = { id: "demo.explode", title: "Explode", export: "explode" };
  const action = (id: string, command: string, label: string, when?: UiContribution["when"]) =>
    ({ location: "topbar/end", id, label, type: "action", command, ...(when ? { when } : {}) }) as UiContribution;

  it("calls the command with its location's context, and hides an action whose when is false", async () => {
    serve([
      extension(
        "demo",
        [
          action("record", "demo.record", "Record", { has: "run" }),
          action("hidden", "demo.record", "Hidden", { eq: ["run.status", "archived"] }),
        ],
        [record],
      ),
    ]);
    const { select } = renderShell();
    const button = await screen.findByTestId("extension-topbar-end-demo:record");
    expect(screen.queryByTestId("extension-topbar-end-demo:hidden")).toBeNull();
    fireEvent.click(button);
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0].context).toMatchObject({
      location: "topbar/end",
      extension: { id: "demo", name: "Extension demo" },
      contribution: "record",
      run: { id: "r1", pipeline: "demo", status: "running" },
      project: { id: "p1", name: "PDO", attributes: { "test/team": "core" } },
    });
    expect(await screen.findByTestId("toast-info")).toHaveTextContent("called on r1");

    // No Run selected: `when: has run` is false, the action goes.
    select(null);
    expect(screen.queryByTestId("extension-topbar-end-demo:record")).toBeNull();
    // An archived Run: the hidden one shows.
    select(runState("r1", "archived"));
    expect(await screen.findByTestId("extension-topbar-end-demo:hidden")).toBeInTheDocument();
  });

  it("reports a failing command in an error toast", async () => {
    serve([extension("demo", [action("explode", "demo.explode", "Explode")], [explode])]);
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    renderShell();
    fireEvent.click(await screen.findByTestId("extension-topbar-end-demo:explode"));
    expect(await screen.findByTestId("toast-error")).toHaveTextContent("“Explode” failed: it broke");
    warn.mockRestore();
  });

  it("leaves out an action whose command the bundle does not export", async () => {
    const missing = { id: "demo.missing", title: "Missing", export: "notThere" };
    serve([
      extension("demo", [action("missing", "demo.missing", "Missing"), page("home", "Demo", "OtherPage")], [missing]),
    ]);
    renderShell();
    await screen.findByTestId("extension-topbar-end-demo:home");
    expect(screen.queryByTestId("extension-topbar-end-demo:missing")).toBeNull();
  });
});

describe("deep links", () => {
  it("open the page on the path they carry", async () => {
    window.history.replaceState(null, "", "/#/ext/demo/topbar/end/note/note-12");
    serve([extension("demo", [page("home", "Demo", "ContextPage")])]);
    renderShell();
    const shell = await screen.findByTestId("extension-page");
    expect(within(shell).getByText("path note/note-12")).toBeInTheDocument();
    expect(screen.getByTestId("extension-page-breadcrumb")).toHaveTextContent("notenote-12");
  });

  it("toward an absent extension say so in an error toast, never a blank page", async () => {
    window.history.replaceState(null, "", "/#/ext/audit/journal");
    serve([extension("demo", [page("home", "Demo", "ContextPage")])]);
    renderShell();
    expect(await screen.findByTestId("toast-error")).toHaveTextContent(
      "Extension “audit” is not installed: this link can't be opened.",
    );
    expect(screen.queryByTestId("extension-page")).toBeNull();
    expect(window.location.hash).toBe("");
  });

  it("toward a location the extension has no page at say so too", async () => {
    window.history.replaceState(null, "", "/#/ext/demo/topbar/start/x");
    serve([extension("demo", [page("home", "Demo", "ContextPage")])]);
    renderShell();
    expect(await screen.findByTestId("toast-error")).toHaveTextContent(
      "The extension Extension demo has no page for this link",
    );
  });

  it("are clickable in a markdown render, written as links or as plain text", async () => {
    serve([extension("demo", [page("home", "Demo", "ContextPage")])]);
    renderShell();
    await screen.findByTestId("extension-topbar-end-demo:home");
    render(
      <Markdown remarkPlugins={REMARK_PLUGINS} components={MARKDOWN_COMPONENTS}>
        {"See [the task](#/ext/demo/topbar/end/task/1) or #/ext/demo/topbar/end/task/2. Not `#/ext/demo/code`."}
      </Markdown>,
    );
    // The artifact viewer the link sits in gives way to the page.
    const dismissViewer = vi.fn();
    const unregister = registerTransientOverlay(dismissViewer);
    fireEvent.click(screen.getByRole("link", { name: "the task" }));
    expect(await screen.findByText("path task/1")).toBeInTheDocument();
    expect(dismissViewer).toHaveBeenCalled();
    unregister();
    fireEvent.keyDown(window, { key: "Escape" });

    const plain = screen.getByRole("link", { name: "#/ext/demo/topbar/end/task/2" });
    expect(plain).toHaveAttribute("href", "#/ext/demo/topbar/end/task/2");
    fireEvent.click(plain);
    expect(await screen.findByText("path task/2")).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "#/ext/demo/code" })).toBeNull();
  });

  it("follow « Back »: the page closes with its link", async () => {
    serve([extension("demo", [page("home", "Demo", "ContextPage")])]);
    renderShell();
    fireEvent.click(await screen.findByTestId("extension-topbar-end-demo:home"));
    await screen.findByTestId("extension-page");
    act(() => {
      window.history.replaceState(null, "", "/");
      window.dispatchEvent(new PopStateEvent("popstate"));
    });
    expect(screen.queryByTestId("extension-page")).toBeNull();
  });
});

describe("the Projet of the selected Run", () => {
  it("is the one its root's repository belongs to, verbatim", () => {
    expect(projectOfRun("r1", runs, projects)?.name).toBe("PDO");
    expect(projectOfRun("r1-child", runs, projects)?.name).toBe("PDO");
    expect(projectOfRun("r2", runs, projects)).toBeNull();
    expect(projectOfRun("unknown", runs, projects)).toBeNull();
  });
});
