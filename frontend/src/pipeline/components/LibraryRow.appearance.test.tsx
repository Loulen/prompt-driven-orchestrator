/**
 * A Pipeline's row with a given Apparence (CONTEXT.md « Élément masquable »): an
 * element hidden is not drawn, its name and its select control always are; the
 * row takes one line when its whole second line is hidden; open and select work
 * the same.
 */
import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ExtensionEntry } from "../../shared/generated/ExtensionEntry";
import type { UiContribution } from "../../shared/generated/UiContribution";
import type { DraftSummary } from "../../shared/generated/DraftSummary";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import ExtensionContextProvider from "../../shared/extensions/ExtensionContextProvider";
import { hostReactMajor } from "../../shared/extensions/shared";
import { resetExtensions, setBundleLoader } from "../../shared/extensions/store";
import { resetAppearance } from "../../hooks/useAppearance";
import { ELEMENTS_STORAGE_KEY } from "../../lib/appearance";
import LibraryRow from "./LibraryRow";
import PipelineItemDecorations from "./PipelineItemDecorations";
import ValidationIndicator from "./ValidationIndicator";

const PIPELINE = {
  id: "implement",
  name: "implement-ticket",
  attributes: {},
} as unknown as PipelineListEntry;

const DRAFT = {
  changes: 3,
  nodes: 4,
  participants: [
    {
      connection: "c1",
      actor: { kind: "local_operator" },
      display: null,
      can_write: true,
      assistant: false,
      selection: null,
      typing: null,
    },
  ],
} as DraftSummary;

const EXTENSION: ExtensionEntry = {
  id: "test",
  name: "Test",
  version: "1.0.0",
  ui: {
    bundle_url: "/ext/test/ui.js",
    react_major: hostReactMajor(),
    ui_module_major: null,
    contributions: [
      { location: "pipeline/item", id: "tag", label: "Tag", type: "decoration", component: "Tag" },
    ] as UiContribution[],
    commands: [],
    locations: [],
  },
};

function appearance(elements: Record<string, boolean>) {
  localStorage.setItem(ELEMENTS_STORAGE_KEY, JSON.stringify(elements));
  resetAppearance();
}

function mount(over: { draft?: unknown } = {}) {
  const on = { onOpen: vi.fn(), onToggleSelect: vi.fn() };
  render(
    <ExtensionContextProvider selectedRun={null} runs={[]} projects={[]}>
      <LibraryRow
        testId="row"
        name="implement-ticket"
        showDuplicate={false}
        onDelete={() => {}}
        deleteTitle="Delete pipeline"
        version={14}
        modified="2026-10-25T12:00:00Z"
        draft={("draft" in over ? over.draft : DRAFT) as never}
        decorations={<PipelineItemDecorations pipeline={PIPELINE} />}
        indicator={<ValidationIndicator validation={{ kind: "all" }} projects={[]} />}
        {...on}
      />
    </ExtensionContextProvider>,
  );
  return on;
}

const row = () => screen.getByTestId("row");
const NO_SECOND_LINE = {
  "pipelines.list.version": false,
  "pipelines.list.draft": false,
  "pipelines.list.modified": false,
  "pipelines.list.ext.test:tag": false,
};

beforeEach(() => {
  localStorage.clear();
  resetAppearance();
  resetExtensions();
  setBundleLoader(async () => ({ Tag: () => <span data-testid="tag">T</span> }));
  vi.stubGlobal(
    "fetch",
    vi.fn(async (url: string) => {
      const body = String(url) === "/ext" ? [EXTENSION] : [];
      return new Response(JSON.stringify(body), { status: 200, headers: { "Content-Type": "application/json" } });
    }),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
  resetExtensions();
});

describe("a Pipeline's row, with a given Apparence", () => {
  it("draws, by default, its version, its draft, its edit date, its validation and the extensions' marks", async () => {
    mount();
    expect(screen.getByTestId("library-row-version")).toHaveTextContent("v14");
    expect(screen.getByTestId("library-row-draft")).toHaveTextContent("draft: 3 changes");
    expect(screen.getByTestId("library-row-participants")).toBeInTheDocument();
    expect(screen.getByTestId("library-row-edited")).toBeInTheDocument();
    expect(screen.getByTestId("library-row-validation")).toHaveTextContent("All");
    expect(await screen.findByTestId("tag")).toBeInTheDocument();
    expect(row()).toHaveAttribute("data-lines", "2");
  });

  it.each([
    ["pipelines.list.version", ["library-row-version"]],
    ["pipelines.list.draft", ["library-row-draft", "library-row-participants"]],
    ["pipelines.list.modified", ["library-row-edited"]],
    ["pipelines.list.validation", ["library-row-validation"]],
  ])("leaves %s out once hidden, and keeps the name and the select control", (id, testIds) => {
    appearance({ [id]: false });
    mount();
    for (const testId of testIds) expect(screen.queryByTestId(testId)).toBeNull();
    expect(screen.getByTestId("library-row-name")).toHaveTextContent("implement-ticket");
    expect(screen.getByRole("checkbox", { name: "Select implement-ticket" })).toBeInTheDocument();
  });

  it("hides an extension's contribution", async () => {
    appearance({ "pipelines.list.ext.test:tag": false });
    mount();
    await act(async () => {});
    expect(screen.queryByTestId("tag")).toBeNull();
    expect(screen.getByTestId("library-row-version")).toBeInTheDocument();
  });

  it("keeps two lines while a contribution of the second line is shown", async () => {
    appearance({ ...NO_SECOND_LINE, "pipelines.list.ext.test:tag": true });
    mount();
    expect(await screen.findByTestId("tag")).toBeInTheDocument();
    expect(row()).toHaveAttribute("data-lines", "2");
  });

  it("takes one line once its whole second line is hidden, whatever the Pipeline holds", async () => {
    appearance(NO_SECOND_LINE);
    mount({ draft: undefined });
    await act(async () => {});
    expect(row()).toHaveAttribute("data-lines", "1");
    expect(screen.queryByTestId("library-row-meta")).toBeNull();
    expect(screen.getByTestId("library-row-validation")).toBeInTheDocument();
  });

  it("on one line, opens and selects as before", async () => {
    appearance(NO_SECOND_LINE);
    const on = mount();
    await act(async () => {});
    fireEvent.click(row());
    fireEvent.keyDown(row(), { key: "Enter" });
    expect(on.onOpen).toHaveBeenCalledTimes(2);
    fireEvent.click(screen.getByRole("checkbox", { name: "Select implement-ticket" }));
    expect(on.onToggleSelect).toHaveBeenCalledOnce();
  });
});
