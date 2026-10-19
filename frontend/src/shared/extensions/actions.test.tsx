/**
 * The Action locations (ADR-0081, amendment #30): each bar and menu of the
 * catalogue renders the extensions' actions, hands its context to their
 * commands, hides an action whose `when` is false, and keeps working when a
 * command fails. Without a contribution, nothing is added.
 */

import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { RunListEntry } from "../generated/RunListEntry";
import type { RunState } from "../generated/RunState";
import type { Trigger } from "../generated/Trigger";
import type { DiffFile } from "../generated/DiffFile";
import type { NodeState } from "../generated/NodeState";
import type { UiContribution } from "../generated/UiContribution";
import type { UiLocation } from "../generated/UiLocation";
import RunRow from "../../run/components/runsList/RunRow";
import RunsBulkActions from "../../run/components/runsList/RunsBulkActions";
import RunsPaneHeader from "../../run/components/runsList/RunsPaneHeader";
import type { RunsPaneState } from "../../run/store/runsList/useRunsPane";
import TriggerRow from "../../trigger/components/triggersList/TriggerRow";
import LibraryRow from "../../pipeline/components/LibraryRow";
import { ContextMenu } from "../../pipeline/components/CanvasContextMenu";
import { canvasMenuExtensions } from "../../pipeline/model/canvasMenuExtensions";
import TabBar from "../../pipeline/components/TabBar";
import { useEditStore } from "../../pipeline/store/editStore";
import type { EditorPipelineDef } from "../../pipeline/model/pipeline";
import type { OpenPipeline } from "../../pipeline/store/editState";
import TreeRowActions from "../../skill/components/TreeRowActions";
import type { SkillBankPanelState } from "../../skill/store/useSkillBankPanel";
import NodeDetailHeader from "../../run/components/nodeDetail/NodeDetailHeader";
import ReviewFileHeader from "../../review/components/ReviewFileHeader";
import Toaster from "../ui/Toaster";
import { resetToasts } from "../ui/toast";
import { TooltipProvider } from "../ui/tooltip";
import type { Project } from "../../workspace/model/project";
import type { CommandContext } from "./contract";
import ExtensionContextProvider from "./ExtensionContextProvider";
import { ExtensionActionButtons } from "./ExtensionActions";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader } from "./store";

const calls: CommandContext[] = [];

/** What an entry says: its text, without its `aria-hidden` icon (a fallback letter). */
function said(element: HTMLElement): string {
  const clone = element.cloneNode(true) as HTMLElement;
  clone.querySelectorAll("[aria-hidden]").forEach((hidden) => hidden.remove());
  return clone.textContent?.trim() ?? "";
}

const bundle = {
  record: (context: CommandContext) => {
    calls.push(context);
  },
  explode: () => {
    throw new Error("it broke");
  },
};

/** One action of `extension` at `location`, calling `command`. */
function action(location: UiLocation, id: string, when?: UiContribution["when"]): UiContribution {
  return {
    location,
    id,
    label: `Do ${id}`,
    type: "action",
    command: `cmd.${id}`,
    ...(when ? { when } : {}),
  } as UiContribution;
}

function extension(id: string, name: string, contributions: UiContribution[]): ExtensionEntry {
  const commands = contributions.map((c) => ({
    id: (c as { command: string }).command,
    title: c.label,
    export: c.id.startsWith("fail") ? "explode" : "record",
  }));
  return {
    id,
    name,
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

function shell(children: ReactNode, selected: RunState | null = null) {
  const runs = [{ run_id: "r1", pipeline_name: "demo", status: "running", effective_repo: "/repos/pdo" }] as unknown as RunListEntry[];
  const projects = [{ id: "p1", name: "PDO", members: ["/repos/pdo"] }] as unknown as Project[];
  return (
    <TooltipProvider>
      <ExtensionContextProvider selectedRun={selected} runs={runs} projects={projects}>
        {children}
        <Toaster />
      </ExtensionContextProvider>
    </TooltipProvider>
  );
}

function runEntry(id: string, status: string): RunListEntry {
  return {
    run_id: id,
    name: `Run ${id}`,
    pipeline_name: "demo",
    status,
    effective_repo: "/repos/pdo",
    stalled: false,
    started_at: null,
    attributes: { "test/started_by": "alice" },
  } as unknown as RunListEntry;
}

/** The Runs pane's state, as far as a row, the header and the bulk bar read it. */
function fakePane(over: Partial<RunsPaneState> = {}): RunsPaneState {
  return {
    isExpanded: () => false,
    setExpanded: vi.fn(),
    runSel: new Set<string>(),
    runSelIds: [],
    renamingRunId: null,
    renameValue: "",
    setRenameValue: vi.fn(),
    renameInputRef: { current: null },
    commitRename: vi.fn(),
    cancelRename: vi.fn(),
    startRename: vi.fn(),
    handlePause: vi.fn(),
    handleResume: vi.fn(),
    setConfirmRetryAll: vi.fn(),
    setConfirmCleanup: vi.fn(),
    setConfirmForget: vi.fn(),
    runShell: { open: vi.fn() },
    runsScrollRef: { current: null },
    selectRange: vi.fn(),
    toggleSel: vi.fn(),
    visibleRunIds: [],
    runById: new Map(),
    filteredRuns: [],
    availableRepos: [],
    setProjectEditor: vi.fn(),
    projectOf: () => null,
    openProjectEditor: vi.fn(),
    runningWillStop: 0,
    cleanupItems: [],
    retryItems: [],
    pauseItems: [],
    setRunBulkKind: vi.fn(),
    clearSel: vi.fn(),
    runBulkKind: null,
    cleanupCascade: { finished: 0, live: 0 },
    handleRunsSettled: vi.fn(),
    bulkShown: { cleanup: true, retry: true, pause: true },
    mayOnRow: () => true,
    ...over,
  } as unknown as RunsPaneState;
}

function renderRow(run: RunListEntry, pane = fakePane(), onSelectRun = vi.fn()) {
  render(
    shell(
      <RunRow
        node={{ run, children: [], depth: 0, counts: { running: 0, awaiting: 0, done: 0, failed: 0 }, sectionParentLink: null } as never}
        pane={pane}
        selectedRunId={null}
        triggers={[]}
        onSelectRun={onSelectRun}
        onShowTriggers={vi.fn()}
      />,
    ),
  );
  return { pane, onSelectRun };
}

beforeEach(() => {
  resetExtensions();
  resetToasts();
  calls.length = 0;
  setBundleLoader(async () => bundle);
  Element.prototype.scrollIntoView = vi.fn();
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("a bar of the catalogue (runs/header)", () => {
  it("shows one icon button per action, before the core's, and calls its command with the context", async () => {
    serve([extension("demo", "Demo", [action("runs/header", "sync")])]);
    render(shell(<RunsPaneHeader pane={fakePane()} onNewRun={vi.fn()} />, { run_id: "r1", pipeline_name: "demo", status: "running" } as RunState));
    const button = await screen.findByTestId("extension-action-demo:sync");
    expect(button).toHaveAccessibleName("Do sync");
    // Before « New Run » in the header's order.
    const buttons = screen.getAllByRole("button");
    expect(buttons.indexOf(button)).toBeLessThan(buttons.indexOf(screen.getByTestId("new-run-button")));
    fireEvent.click(button);
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "runs/header",
      run: { id: "r1" },
      project: { name: "PDO" },
      extension: { id: "demo", name: "Demo" },
      contribution: "sync",
    });
  });

  it("adds nothing without a contribution", async () => {
    serve([]);
    render(shell(<RunsPaneHeader pane={fakePane()} onNewRun={vi.fn()} />));
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(screen.queryByTestId("extension-actions-runs/header")).toBeNull();
  });

  it("renders only actions, and not those of another location", async () => {
    serve([
      extension("demo", "Demo", [
        action("pipelines/header", "elsewhere"),
        { location: "runs/header", id: "page", label: "P", type: "page", component: "record" } as UiContribution,
        action("runs/header", "here"),
      ]),
    ]);
    render(shell(<ExtensionActionButtons location="runs/header" extra={{}} />));
    await screen.findByTestId("extension-action-demo:here");
    expect(screen.getAllByRole("button")).toHaveLength(1);
  });
});

describe("the « ⋯ » menu of a Run's row (run/item/menu)", () => {
  const notArchived = { not: { eq: ["entity.status", "archived"] } } as UiContribution["when"];

  it("lists the row's verbs, then a group per extension titled with its name, before the destructive entry", async () => {
    serve([
      extension("first", "First", [action("run/item/menu", "a"), action("run/item/menu", "b")]),
      extension("second", "Second", [action("run/item/menu", "c")]),
    ]);
    renderRow(runEntry("r1", "completed"));
    fireEvent.click(screen.getByTestId("run-row-menu"));
    const menu = await screen.findByTestId("run-row-menu-content");
    await within(menu).findByTestId("extension-action-first:a");
    const items = within(menu).getAllByRole("menuitem").map(said);
    expect(items).toEqual([
      "Rename",
      "Retry all",
      "Open shell",
      "Copy run id",
      "Do a",
      "Do b",
      "Do c",
      "Cleanup",
    ]);
    expect(within(screen.getByTestId("extension-menu-run/item/menu-first")).getByText("First")).toBeInTheDocument();
    expect(within(screen.getByTestId("extension-menu-run/item/menu-second")).getByText("Second")).toBeInTheDocument();
  });

  it("hands the row's Run over as `entity`, without selecting the row", async () => {
    serve([extension("demo", "Demo", [action("run/item/menu", "link")])]);
    const { onSelectRun } = renderRow(runEntry("r7", "running"));
    fireEvent.click(screen.getByTestId("run-row-menu"));
    fireEvent.click(await screen.findByTestId("extension-action-demo:link"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "run/item/menu",
      entity: {
        kind: "run",
        id: "r7",
        name: "Run r7",
        pipeline: "demo",
        status: "running",
        attributes: { "test/started_by": "alice" },
      },
    });
    expect(onSelectRun).not.toHaveBeenCalled();
  });

  it("hides an action whose `when` is false on the row's Run", async () => {
    serve([extension("demo", "Demo", [action("run/item/menu", "live", notArchived), action("run/item/menu", "always")])]);
    renderRow(runEntry("r1", "archived"));
    fireEvent.click(screen.getByTestId("run-row-menu"));
    await screen.findByTestId("extension-action-demo:always");
    expect(screen.queryByTestId("extension-action-demo:live")).toBeNull();
    expect(screen.getByTestId("run-menu-forget")).toBeInTheDocument();
  });

  it("a failing command shows an error toast, and the menu keeps working", async () => {
    serve([extension("demo", "Demo", [action("run/item/menu", "fail"), action("run/item/menu", "ok")])]);
    renderRow(runEntry("r1", "running"));
    fireEvent.click(screen.getByTestId("run-row-menu"));
    fireEvent.click(await screen.findByTestId("extension-action-demo:fail"));
    expect(await screen.findByTestId("toast-error")).toHaveTextContent("“Do fail” failed: it broke");
    fireEvent.click(screen.getByTestId("run-row-menu"));
    fireEvent.click(await screen.findByTestId("extension-action-demo:ok"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0].contribution).toBe("ok");
  });

  it("its core entries do what the hover icons do", async () => {
    serve([]);
    const { pane } = renderRow(runEntry("r1", "running"));
    fireEvent.click(screen.getByTestId("run-row-menu"));
    fireEvent.click(await screen.findByTestId("run-menu-pause"));
    expect(pane.handlePause).toHaveBeenCalledWith("r1");
    fireEvent.click(screen.getByTestId("run-row-menu"));
    fireEvent.click(await screen.findByTestId("run-menu-cleanup"));
    expect(pane.setConfirmCleanup).toHaveBeenCalledWith({ runId: "r1", status: "running" });
    expect(screen.queryByTestId("run-menu-retry-all")).toBeNull();
  });
});

describe("the multi-selection bar (runs/bulk)", () => {
  it("adds the extensions' entries after the core's, called with the selected ids", async () => {
    serve([extension("demo", "Demo", [action("runs/bulk", "tag")])]);
    render(shell(<RunsBulkActions pane={fakePane({ runSelIds: ["r1", "r3"] })} />));
    const entry = await screen.findByTestId("bulk-action-demo:tag");
    const keys = screen
      .getAllByRole("button")
      .map((b) => b.getAttribute("data-testid"))
      .filter((id) => id?.startsWith("bulk-action-"));
    expect(keys).toEqual(["bulk-action-cleanup", "bulk-action-retry", "bulk-action-pause", "bulk-action-demo:tag"]);
    fireEvent.click(entry);
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({ location: "runs/bulk", selection: { kind: "run", ids: ["r1", "r3"] } });
  });
});

describe("the « ⋯ » menus of the Trigger and Pipeline rows", () => {
  it("trigger/item/menu hands the Trigger over, before Delete", async () => {
    serve([extension("demo", "Demo", [action("trigger/item/menu", "t")])]);
    const onDelete = vi.fn();
    const trigger = {
      id: "t1",
      name: "Nightly",
      pipeline_name: "demo",
      cron: "0 3 * * *",
      enabled: false,
      last_outcome: null,
      attributes: {},
    } as unknown as Trigger;
    const onSelect = vi.fn();
    render(
      shell(
        <TriggerRow
          trigger={trigger}
          isSelected={false}
          rowSelected={false}
          allRepos={[]}
          onSelect={onSelect}
          onSelectControl={vi.fn()}
          onDelete={onDelete}
          onToggle={vi.fn()}
        />,
      ),
    );
    fireEvent.click(screen.getByTestId("trigger-row-menu"));
    const menu = await screen.findByTestId("trigger-row-menu-content");
    await within(menu).findByTestId("extension-action-demo:t");
    expect(within(menu).getAllByRole("menuitem").map(said)).toEqual([
      "Enable",
      "Copy trigger id",
      "Do t",
      "Delete",
    ]);
    fireEvent.click(screen.getByTestId("extension-action-demo:t"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "trigger/item/menu",
      entity: { kind: "trigger", id: "t1", name: "Nightly", pipeline: "demo", enabled: false },
    });
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("pipeline/item/menu hands the Pipeline over; a row without it has no menu", async () => {
    serve([extension("demo", "Demo", [action("pipeline/item/menu", "p")])]);
    const onDelete = vi.fn();
    const { rerender } = render(
      shell(
        <LibraryRow
          name="Deploy"
          nodeCount={3}
          showDuplicate
          onOpen={vi.fn()}
          onDuplicate={vi.fn()}
          onDelete={onDelete}
          deleteTitle="Delete pipeline"
          menuEntity={{ kind: "pipeline", id: "deploy", name: "Deploy", attributes: {} }}
        />,
      ),
    );
    fireEvent.click(screen.getByTestId("pipeline-row-menu"));
    fireEvent.click(await screen.findByTestId("extension-action-demo:p"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({ location: "pipeline/item/menu", entity: { kind: "pipeline", id: "deploy" } });
    fireEvent.click(screen.getByTestId("pipeline-row-menu"));
    fireEvent.click(await screen.findByTestId("pipeline-menu-delete"));
    expect(onDelete).toHaveBeenCalledTimes(1);

    rerender(
      shell(<LibraryRow name="Lib" nodeCount={1} showDuplicate={false} onDelete={vi.fn()} deleteTitle="Delete" />),
    );
    expect(screen.queryByTestId("pipeline-row-menu")).toBeNull();
  });
});

describe("the canvas's right-click menus", () => {
  const pipeline = {
    name: "demo",
    variables: {},
    nodes: [{ id: "coder", name: "Coder", type: "agent", inputs: [], outputs: [], interactive: false }],
    edges: [{ source: { node: "start", port: "prompt" }, target: { node: "coder", port: "task" } }],
  } as unknown as EditorPipelineDef;
  const handlers = {
    onDeleteNode: vi.fn(),
    onDeleteNote: vi.fn(),
    onDuplicateNode: vi.fn(),
    onExportNode: vi.fn(),
    onFanOut: vi.fn(),
    onDeleteEdge: vi.fn(),
    onClose: vi.fn(),
  };

  it("canvas/node/menu hands the node and its pipeline over, before Delete", async () => {
    serve([extension("demo", "Demo", [action("canvas/node/menu", "n")])]);
    const run = { run_id: "r1", nodes: { coder: { status: "completed" } } } as unknown as RunState;
    const menu = { x: 10, y: 10, type: "node" as const, id: "coder" };
    render(shell(<ContextMenu {...menu} {...handlers} extensions={canvasMenuExtensions(menu, pipeline, "demo-tab", run)} />));
    const entry = await screen.findByTestId("extension-action-demo:n");
    const items = within(screen.getByTestId("canvas-context-menu")).getAllByRole("menuitem");
    expect(items.map(said)).toEqual(["Duplicate", "Export as YAML…", "Do n", "Delete"]);
    fireEvent.click(entry);
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "canvas/node/menu",
      entity: { kind: "node", id: "coder", name: "Coder", type: "agent", status: "completed" },
      pipeline: { id: "demo-tab", name: "demo", run: "r1" },
    });
  });

  it("canvas/edge/menu hands the edge over, before Delete edge", async () => {
    serve([extension("demo", "Demo", [action("canvas/edge/menu", "e")])]);
    const menu = { x: 10, y: 10, type: "edge" as const, id: "e-0", edgeIndex: 0 };
    render(shell(<ContextMenu {...menu} {...handlers} extensions={canvasMenuExtensions(menu, pipeline, "demo", null)} />));
    fireEvent.click(await screen.findByTestId("extension-action-demo:e"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "canvas/edge/menu",
      entity: { kind: "edge", id: "start.prompt->coder.task", from: "start", to: "coder" },
      pipeline: { id: "demo", run: null },
    });
    expect(screen.getByText("Delete edge")).toBeInTheDocument();
  });

  it("keeps its entries, and no separator, without a contribution", async () => {
    serve([]);
    const menu = { x: 10, y: 10, type: "node" as const, id: "coder" };
    render(shell(<ContextMenu {...menu} {...handlers} extensions={canvasMenuExtensions(menu, pipeline, "demo", null)} />));
    const content = await screen.findByTestId("canvas-context-menu");
    await waitFor(() => expect(fetch).toHaveBeenCalled());
    expect(within(content).getAllByRole("menuitem").map((i) => i.textContent)).toEqual([
      "Duplicate",
      "Export as YAML…",
      "Delete",
    ]);
    expect(within(content).queryByRole("separator")).toBeNull();
  });
});

describe("the tab bar's menu (tab/menu)", () => {
  it("adds the extensions' groups after the close entries, handed the tab's pipeline", async () => {
    serve([extension("demo", "Demo", [action("tab/menu", "tab")])]);
    const tab = (id: string): OpenPipeline =>
      ({
        id,
        scope: "repo",
        pipeline: { name: `Pipeline ${id}`, version: "1.0", variables: {}, nodes: [], edges: [] },
        prompts: {},
        diagnostics: [],
        dirty: false,
        externalDirty: false,
      }) as OpenPipeline;
    useEditStore.setState({ openTabs: [tab("a"), tab("b")], activeTabId: "a", lastSavedAt: {} });
    render(shell(<TabBar />));
    fireEvent.contextMenu(screen.getByTestId("tab-title-b").closest("button")!);
    fireEvent.click(await screen.findByTestId("extension-action-demo:tab"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({ location: "tab/menu", entity: { kind: "pipeline", id: "b", name: "Pipeline b" } });
    // The action closed nothing.
    expect(useEditStore.getState().openTabs).toHaveLength(2);
  });
});

describe("the skill bank's menu (skill/menu)", () => {
  it("hands the skill or the folder over, before its delete", async () => {
    serve([extension("demo", "Demo", [action("skill/menu", "s")])]);
    const panel = { menuFor: { kind: "skill", id: "tdd" }, setMenuFor: vi.fn(), setMovePickerFor: vi.fn() } as unknown as SkillBankPanelState;
    render(
      shell(
        <TreeRowActions
          row={{ ref: { kind: "skill", id: "tdd" }, depth: 0, skill: { id: "tdd", name: "tdd" } } as never}
          panel={panel}
        />,
      ),
    );
    const menu = await screen.findByTestId("tree-menu");
    const entry = await within(menu).findByTestId("extension-action-demo:s");
    expect(within(menu).getAllByRole("menuitem").map(said)).toEqual([
      expect.stringContaining("Rename"),
      expect.stringContaining("Move to…"),
      expect.stringContaining("Copy id"),
      expect.stringContaining("Export"),
      "Do s",
      expect.stringContaining("Delete…"),
    ]);
    fireEvent.click(entry);
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({ location: "skill/menu", entity: { kind: "skill", id: "tdd", name: "tdd" } });
  });
});

describe("the node's and the Review's bars", () => {
  it("node/toolbar hands the node of the detail over", async () => {
    serve([extension("demo", "Demo", [action("node/toolbar", "node")])]);
    const node = { node_id: "coder", status: "running", iter: 1, iterations: [] } as unknown as NodeState;
    render(shell(<NodeDetailHeader node={node} nodeName="Coder" hasMultipleIters={false} selectedIter={1} onSelectIter={vi.fn()} />));
    fireEvent.click(await screen.findByTestId("extension-action-demo:node"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "node/toolbar",
      entity: { kind: "node", id: "coder", name: "Coder", status: "running" },
    });
  });

  it("review/file hands the file and the pair over, without toggling the file", async () => {
    serve([extension("demo", "Demo", [action("review/file", "file")])]);
    const onToggle = vi.fn();
    const file = {
      old_path: null,
      new_path: "src/lib.rs",
      status: "modified",
      additions: 1,
      deletions: 0,
      binary: false,
      hunks: [],
    } as unknown as DiffFile;
    render(
      shell(
        <ReviewFileHeader
          runId="r1"
          file={file}
          pair={{ from: "fork", to: "tip" }}
          collapsed={false}
          onToggle={onToggle}
          nDrafts={0}
          nOutdated={0}
          sentStates={{ open: 0, proposed: 0, resolved: 0 }}
        />,
      ),
    );
    fireEvent.click(await screen.findByTestId("extension-action-demo:file"));
    await waitFor(() => expect(calls).toHaveLength(1));
    expect(calls[0]).toMatchObject({
      location: "review/file",
      entity: { kind: "file", id: "src/lib.rs", path: "src/lib.rs", status: "modified" },
      review: { from: "fork", to: "tip" },
    });
    expect(onToggle).not.toHaveBeenCalled();
  });
});
