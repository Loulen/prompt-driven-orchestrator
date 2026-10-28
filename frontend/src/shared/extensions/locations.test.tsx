/**
 * The locations #31 added to the catalogue (ADR-0081): tabs, sections (and how
 * they take part in their host's Save and Cancel), decorations and list filters.
 */

import { act, fireEvent, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useEffect, useState } from "react";
import type { ExtensionEntry } from "../generated/ExtensionEntry";
import type { UiContribution } from "../generated/UiContribution";
import type { TriggerListEntry } from "../generated/TriggerListEntry";
import TriggersListPanel from "../../trigger/components/TriggersListPanel";
import TriggerDetailPanel from "../../trigger/components/TriggerDetailPanel";
import type { Trigger } from "../generated/Trigger";
import type { Project } from "../../workspace/model/project";
import type { DecorationContext, DecorationProps, ListFilterProps, SectionProps, TabProps } from "./contract";
import ExtensionDecorations from "./ExtensionDecorations";
import ExtensionSections from "./ExtensionSections";
import { useSectionForm } from "./sectionForm";
import { hostReactMajor } from "./shared";
import { resetExtensions, setBundleLoader, type BundleModule } from "./store";

vi.mock("../../trigger/api/triggers", () => ({
  deleteTrigger: vi.fn(),
  updateTrigger: vi.fn(),
  fetchTriggerFires: vi.fn().mockResolvedValue([]),
}));
vi.mock("../../settings/api/settings", () => ({
  fetchSettings: vi.fn().mockResolvedValue({}),
}));

function extension(id: string, contributions: UiContribution[]): ExtensionEntry {
  return {
    id,
    name: `Extension ${id}`,
    version: "1.0.0",
    ui: {
      bundle_url: `/ext/${id}/ui.js`,
      react_major: hostReactMajor(),
      ui_module_major: null,
      contributions,
      commands: [],
      locations: [],
    },
  };
}

function at(location: string, type: string, id: string, component: string, when?: unknown): UiContribution {
  return { location, id, label: id, type, component, ...(when ? { when } : {}) } as UiContribution;
}

const saves: unknown[] = [];
const cancels: string[] = [];

const bundle: BundleModule = {
  Badge: ({ entity, context }: DecorationProps) => (
    <span data-testid="badge">
      {context.location}:{entity?.id ?? "none"}
    </span>
  ),
  EntityTab: ({ entity, context }: TabProps) => (
    <p>{`${context.location}: ${entity.kind} ${entity.id} ${entity.name} ${entity.attributes["test/started_by"]}`}</p>
  ),
  Boom: () => {
    throw new Error("boom");
  },
  // API 0.9: provides `{ [key]: draft }` before its host submits.
  ProvidingSection: ({ form }: SectionProps) => {
    const [draft, setDraft] = useState("typed");
    useEffect(() => form?.provide?.(() => ({ k: draft })), [form, draft]);
    return <input aria-label="provided" value={draft} onChange={(e) => setDraft(e.target.value)} />;
  },
  RefusingProvider: ({ form }: SectionProps) => {
    useEffect(() => form?.provide?.(() => Promise.reject(new Error("pick a task first"))), [form]);
    return <p>refusing provider</p>;
  },
  NothingProvider: ({ form }: SectionProps) => {
    useEffect(() => form?.provide?.(() => undefined), [form]);
    return <p>nothing provider</p>;
  },
  DraftSection: ({ entity, form }: SectionProps) => {
    const [draft, setDraft] = useState("");
    useEffect(() => {
      if (!form) return undefined;
      const off = form.onSave((saved) => {
        saves.push({ saved, draft });
      });
      const offCancel = form.onCancel(() => {
        cancels.push(draft);
      });
      return () => {
        off();
        offCancel();
      };
    }, [form, draft]);
    return (
      <label>
        {`section of ${entity?.name ?? "?"}`}
        <input aria-label="draft" value={draft} onChange={(e) => setDraft(e.target.value)} />
      </label>
    );
  },
  FailingSection: ({ form }: SectionProps) => {
    useEffect(() => form?.onSave(() => Promise.reject(new Error("refused by the section"))), [form]);
    return <p>failing section</p>;
  },
  StartedBy: ({ filter, setFilter, resource, label }: ListFilterProps) => (
    <button type="button" title={label} onClick={() => setFilter({ ...filter, "test/started_by": "alice" })}>
      only alice ({resource})
    </button>
  ),
};

function serve(extensions: ExtensionEntry[]) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) =>
      url === "/ext"
        ? new Response(JSON.stringify(extensions), { status: 200, headers: { "Content-Type": "application/json" } })
        : new Response("not found", { status: 404 }),
    ),
  );
}

beforeEach(() => {
  resetExtensions();
  saves.length = 0;
  cancels.length = 0;
  setBundleLoader(async () => bundle);
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

const RUN = { entity: { kind: "run", id: "r1", attributes: { "test/note": "note-1" } } };

describe("a decoration location", () => {
  it("renders nothing at all without a contribution", async () => {
    serve([extension("demo", [at("trigger/item", "decoration", "b", "Badge")])]);
    const { container } = render(
      <div data-testid="row">
        <ExtensionDecorations<DecorationContext> location="run/item" extra={RUN} />
      </div>,
    );
    await act(async () => {});
    expect(screen.getByTestId("row")).toBeEmptyDOMElement();
    expect(container.querySelector("[data-testid^=extension-decorations]")).toBeNull();
  });

  it("marks the item once per contribution, in registration order, with its entity and when", async () => {
    serve([
      extension("first", [at("run/item", "decoration", "b", "Badge")]),
      extension("second", [
        at("run/item", "decoration", "b", "Badge", { has: "entity.attributes.test/note" }),
        at("run/item", "decoration", "never", "Badge", { has: "entity.attributes.test/nothing" }),
      ]),
    ]);
    render(<ExtensionDecorations<DecorationContext> location="run/item" extra={RUN} />);
    await waitFor(() => expect(screen.getAllByTestId("badge")).toHaveLength(2));
    const marks = screen.getByTestId("extension-decorations-run/item");
    expect(within(marks).getByTestId("extension-decoration-first:b")).toHaveTextContent("run/item:r1");
    expect(within(marks).getByTestId("extension-decoration-second:b")).toBeInTheDocument();
    expect(within(marks).queryByTestId("extension-decoration-second:never")).toBeNull();
  });

  it("keeps a failing decoration to its own mark", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    vi.spyOn(console, "warn").mockImplementation(() => {});
    serve([
      extension("demo", [
        at("statusbar", "decoration", "boom", "Boom"),
        at("statusbar", "decoration", "ok", "Badge"),
      ]),
    ]);
    render(<ExtensionDecorations<DecorationContext> location="statusbar" extra={{ entity: null }} />);
    expect(await screen.findByTestId("badge")).toHaveTextContent("statusbar:none");
    const failed = screen.getByTestId("extension-failed-demo");
    expect(failed).toHaveTextContent("!");
    expect(failed).toHaveAttribute("title", "This element of the extension demo failed.");
  });
});

describe("the review/file location, which takes actions and decorations", () => {
  const FILE = {
    entity: { kind: "file", id: "src/a.ts", name: "a.ts", path: "src/a.ts", status: "modified", attributes: {} },
    file: { run: "r1", path: "src/a.ts", status: "modified" },
    review: { from: "main", to: "r1" },
  };
  function withAction(contributions: UiContribution[]): ExtensionEntry {
    const entry = extension("demo", [
      ...contributions,
      { location: "review/file", id: "open", label: "Open", type: "action", command: "demo.open" } as UiContribution,
    ]);
    entry.ui!.commands = [{ id: "demo.open", title: "Open", export: "open" }] as NonNullable<ExtensionEntry["ui"]>["commands"];
    return entry;
  }

  beforeEach(() => setBundleLoader(async () => ({ ...bundle, open: () => {} })));

  it("renders its decorations, not its actions", async () => {
    serve([withAction([at("review/file", "decoration", "b", "Badge")])]);
    render(<ExtensionDecorations<DecorationContext> location="review/file" extra={FILE} />);
    expect(await screen.findByTestId("badge")).toHaveTextContent("review/file:src/a.ts");
    const marks = screen.getByTestId("extension-decorations-review/file");
    expect(within(marks).getAllByTestId(/^extension-decoration-/)).toHaveLength(1);
  });

  it("renders nothing at all when it holds an action only", async () => {
    serve([withAction([])]);
    const { container } = render(
      <div data-testid="header">
        <ExtensionDecorations<DecorationContext> location="review/file" extra={FILE} />
      </div>,
    );
    await act(async () => {});
    expect(screen.getByTestId("header")).toBeEmptyDOMElement();
    expect(container.querySelector("[data-testid^=extension-decorations]")).toBeNull();
  });
});

/** A host with a Save and a Cancel, like the Projet editor. */
function Host({ onSaved }: { onSaved: (error: string | null) => void }) {
  const sections = useSectionForm();
  const extra = { entity: { kind: "project", id: null, name: "PDO", attributes: {} }, creating: true };
  return (
    <div>
      <ExtensionSections location="project/edit" extra={extra} form={sections.form} />
      <button
        type="button"
        onClick={() =>
          sections
            .save({ kind: "project", id: "p-new", name: "PDO", attributes: {} })
            .then(() => onSaved(null), (e: Error) => onSaved(e.message))
        }
      >
        Save
      </button>
      <button type="button" onClick={() => sections.cancel()}>
        Cancel
      </button>
    </div>
  );
}

describe("a section location", () => {
  it("renders every extension's section, which applies its draft only on its host's Save, with the saved id", async () => {
    serve([
      extension("demo", [at("project/edit", "section", "attach", "DraftSection")]),
      extension("rights", [at("project/edit", "section", "rights", "DraftSection")]),
    ]);
    const done = vi.fn();
    render(<Host onSaved={done} />);
    await waitFor(() => expect(screen.getAllByText("section of PDO")).toHaveLength(2));
    const [demo, rights] = screen.getAllByTestId(/^extension-section-/);
    expect(demo).toHaveAttribute("data-testid", "extension-section-demo:attach");
    expect(rights).toHaveAttribute("data-testid", "extension-section-rights:rights");
    expect(within(demo).getByText("· Extension demo")).toBeInTheDocument();

    fireEvent.change(within(demo).getByLabelText("draft"), { target: { value: "draft-1" } });
    expect(saves).toEqual([]);
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(done).toHaveBeenCalledWith(null));
    expect(saves).toEqual([
      { saved: { kind: "project", id: "p-new", name: "PDO", attributes: {} }, draft: "draft-1" },
      { saved: { kind: "project", id: "p-new", name: "PDO", attributes: {} }, draft: "" },
    ]);
  });

  it("rejects the host's save when a section fails, and runs the cancels on Cancel", async () => {
    serve([
      extension("demo", [
        at("project/edit", "section", "attach", "DraftSection"),
        at("project/edit", "section", "fails", "FailingSection"),
      ]),
    ]);
    const done = vi.fn();
    render(<Host onSaved={done} />);
    await screen.findByText("failing section");
    fireEvent.click(screen.getByText("Save"));
    await waitFor(() => expect(done).toHaveBeenCalledWith("refused by the section"));
    fireEvent.click(screen.getByText("Cancel"));
    expect(cancels).toEqual([""]);
  });

  it("loads the saved entity only when a section takes part in the Save", async () => {
    serve([extension("demo", [at("project/edit", "section", "attach", "DraftSection")])]);
    const load = vi.fn(async () => ({ kind: "run", id: "r-new", attributes: { "test/started_by": "alice" } }));
    let host: ReturnType<typeof useSectionForm> | null = null;
    const keep = (sections: ReturnType<typeof useSectionForm>) => {
      host = sections;
    };
    function LazyHost({ withSection }: { withSection: boolean }) {
      const sections = useSectionForm();
      useEffect(() => keep(sections), [sections]);
      return withSection ? (
        <ExtensionSections location="project/edit" extra={{ entity: null, creating: true }} form={sections.form} />
      ) : null;
    }
    const { rerender } = render(<LazyHost withSection={false} />);
    await act(async () => {});
    await host!.save(load);
    expect(load).not.toHaveBeenCalled();

    rerender(<LazyHost withSection />);
    await screen.findByLabelText("draft");
    await host!.save(load);
    expect(load).toHaveBeenCalledTimes(1);
    expect(saves).toEqual([
      { saved: { kind: "run", id: "r-new", attributes: { "test/started_by": "alice" } }, draft: "" },
    ]);
  });

  it("gives no form where the host has none", async () => {
    serve([extension("demo", [at("run/info", "section", "info", "DraftSection")])]);
    render(
      <ExtensionSections location="run/info" extra={{ entity: { kind: "run", id: "r1", name: "Run", attributes: {} } }} />,
    );
    expect(await screen.findByText("section of Run")).toBeInTheDocument();
  });
});

describe("a run/new section that provides a value (API 0.9)", () => {
  function ProvidingHost({ keep }: { keep: (host: ReturnType<typeof useSectionForm>) => void }) {
    const sections = useSectionForm();
    useEffect(() => {
      keep(sections);
    }, [sections, keep]);
    return <ExtensionSections location="run/new" extra={{ mode: "run" }} form={sections.form} />;
  }

  it("places each value under its section's extension, merging one extension's objects", async () => {
    serve([
      extension("acme", [
        at("run/new", "section", "first", "ProvidingSection"),
        at("run/new", "section", "second", "NothingProvider"),
      ]),
      extension("other", [at("run/new", "section", "third", "DraftSection")]),
    ]);
    let host: ReturnType<typeof useSectionForm> | null = null;
    render(<ProvidingHost keep={(h) => (host = h)} />);
    await screen.findByText("nothing provider");
    fireEvent.change(screen.getByLabelText("provided"), { target: { value: "T-1" } });
    await waitFor(async () =>
      expect(await host!.provided()).toEqual({ acme: { k: "T-1" } }),
    );
  });

  it("provides nothing without a providing section", async () => {
    let host: ReturnType<typeof useSectionForm> | null = null;
    serve([extension("other", [at("run/new", "section", "third", "DraftSection")])]);
    render(<ProvidingHost keep={(h) => (host = h)} />);
    await screen.findByLabelText("draft");
    await expect(host!.provided()).resolves.toEqual({});
  });

  it("rejects when a provider fails", async () => {
    let host: ReturnType<typeof useSectionForm> | null = null;
    serve([extension("acme", [at("run/new", "section", "refuses", "RefusingProvider")])]);
    render(<ProvidingHost keep={(h) => (host = h)} />);
    await screen.findByText("refusing provider");
    await expect(host!.provided()).rejects.toThrow("pick a task first");
  });

  it("merges the objects two sections of one extension provide", () => {
    const host = renderHook(() => useSectionForm()).result.current;
    host.form.provide(() => ({ a: 1 }), 0, "acme");
    host.form.provide(() => ({ b: 2 }), 1, "acme");
    host.form.provide(() => "flat", 2, "other");
    return expect(host.provided()).resolves.toEqual({ acme: { a: 1, b: 2 }, other: "flat" });
  });
});

describe("the list filters of the Triggers list", () => {
  function trigger(id: string, repo: string, projectId: string, attributes?: Record<string, string>): TriggerListEntry {
    return {
      id, name: `Trigger ${id}`, pipeline_id: "p", pipeline_name: "P", target_repo: repo,
      project_id: projectId,
      effective_repo: repo, variables: {}, cron: "0 9 * * *", overlap_policy: "skip", auto_name: true,
      enabled: true, ...(attributes ? { attributes } : {}),
    } as unknown as TriggerListEntry;
  }
  // ADR-0087: each Trigger is in the Projet it launches in.
  const triggers = [
    trigger("t1", "/repos/a", "pa", { "test/started_by": "alice" }),
    trigger("t2", "/repos/b", "pb", { "test/started_by": "bob" }),
    trigger("t3", "/repos/b", "pb"),
  ];
  const projects = [
    { id: "pa", name: "A", members: ["/repos/a"], attributes: {} },
    { id: "pb", name: "B", members: ["/repos/b"], attributes: { "test/started_by": "alice" } },
  ] as unknown as Project[];

  function names() {
    return screen.getAllByTestId("trigger-row").map((r) => within(r).getByText(/^Trigger /).textContent);
  }

  it("keep the Triggers carrying the attributes (triggers/filter), or whose Projet does (projects/filter)", async () => {
    serve([
      extension("test", [
        at("triggers/filter", "list_filter", "by", "StartedBy"),
        at("projects/filter", "list_filter", "by-project", "StartedBy"),
      ]),
    ]);
    render(
      <TriggersListPanel
        triggers={triggers}
        projects={projects}
        selectedTriggerId={null}
        onSelectTrigger={() => {}}
        onNewTrigger={() => {}}
        onTriggersChanged={() => {}}
      />,
    );
    expect(names()).toEqual(["Trigger t1", "Trigger t2", "Trigger t3"]);
    // Each control gets its declared label; only the Projet's row is tagged.
    expect(await screen.findByText("only alice (trigger)")).toHaveAttribute("title", "by");
    expect(screen.getByText("only alice (project)")).toHaveAttribute("title", "by-project");
    expect(within(screen.getByTestId("extension-filters-project")).getByTestId("extension-filters-project-tag")).toHaveTextContent("Project");
    expect(within(screen.getByTestId("extension-filters-trigger")).queryByTestId("extension-filters-project-tag")).toBeNull();
    fireEvent.click(await screen.findByText("only alice (trigger)"));
    expect(names()).toEqual(["Trigger t1"]);
    fireEvent.click(screen.getByText("only alice (project)"));
    // t1's Projet (A) does not carry it.
    expect(screen.queryAllByTestId("trigger-row")).toEqual([]);
    expect(screen.getByTestId("trigger-filter-empty")).toBeInTheDocument();
    fireEvent.click(screen.getByText("Clear filters"));
    fireEvent.click(screen.getByText("only alice (project)"));
    expect(names()).toEqual(["Trigger t2", "Trigger t3"]);
  });
});

describe("the trigger/item decorations", () => {
  it("sit at the end of the name's line, never on the pipeline's name line", async () => {
    const t = {
      id: "t1", name: "Nightly", pipeline_id: "p", pipeline_name: "fp-demo", target_repo: "/repos/a",
      effective_repo: "/repos/a", variables: {}, cron: "0 9 * * *", overlap_policy: "skip", auto_name: true,
      enabled: true,
    } as unknown as TriggerListEntry;
    const panel = (
      <TriggersListPanel
        triggers={[t]}
        projects={[]}
        selectedTriggerId={null}
        onSelectTrigger={() => {}}
        onNewTrigger={() => {}}
        onTriggersChanged={() => {}}
      />
    );
    serve([
      extension("one", [at("trigger/item", "decoration", "b", "Badge")]),
      extension("two", [at("trigger/item", "decoration", "b", "Badge")]),
    ]);
    render(panel);
    await waitFor(() => expect(screen.getAllByTestId("badge")).toHaveLength(2));
    const marks = screen.getByTestId("extension-decorations-trigger/item");
    const nameLine = screen.getByText("fp-demo").parentElement!;
    expect(nameLine.contains(marks)).toBe(false);
    // On one line at the end of the row's first line: the row keeps its 46 px.
    expect(marks.closest(".lp-deco")).not.toBeNull();
    expect(marks.className).toContain("flex-nowrap");
  });
});

describe("a tab location", () => {
  const trigger = {
    id: "trg-1", name: "Nightly", pipeline_id: "p", pipeline_name: "P", variables: {},
    cron: "0 9 * * *", overlap_policy: "skip", auto_name: true, enabled: true,
    attributes: { "test/started_by": "alice" },
  } as unknown as Trigger;

  it("adds no tab bar to a Trigger's detail without a contribution", async () => {
    serve([extension("demo", [at("node/detail", "tab", "t", "EntityTab")])]);
    render(<TriggerDetailPanel trigger={trigger} onSelectRun={() => {}} />);
    await act(async () => {});
    expect(screen.queryByTestId("trigger-tab-details")).toBeNull();
  });

  it("adds the extension's tab after Details, with the Trigger", async () => {
    serve([extension("demo", [at("trigger/detail", "tab", "t", "EntityTab")])]);
    render(<TriggerDetailPanel trigger={trigger} onSelectRun={() => {}} />);
    fireEvent.click(await screen.findByTestId("trigger-tab-demo:t"));
    expect(screen.getByText("trigger/detail: trigger trg-1 Nightly alice")).toBeInTheDocument();
    expect(screen.queryByText("Configuration")).toBeNull();
    fireEvent.click(screen.getByTestId("trigger-tab-details"));
    expect(screen.getByText("Configuration")).toBeInTheDocument();
  });
});
