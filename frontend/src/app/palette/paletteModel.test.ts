import { Play } from "lucide-react";
import { describe, expect, it } from "vitest";
import type { SourceState } from "../../shared/extensions/palette";
import { buildSections, flatRows, parseQuery, RESULT_CAP, type BuildInput, type PaletteItem } from "./paletteModel";

const icons = { selection: Play, recent: Play, commands: Play, runs: Play, pipelines: Play, triggers: Play };

const item = (group: string, label: string, extra: Partial<PaletteItem> = {}): PaletteItem => ({
  id: `${group}:${label}`,
  group,
  label,
  lead: { kind: "icon", icon: Play },
  run: () => {},
  ...extra,
});

const ok = (n: number, title = "search hit"): SourceState => ({
  status: "ok",
  total: n,
  results: Array.from({ length: n }, (_, i) => ({ id: String(i), title: `${title} ${i}`, link: `#/ext/a/topbar/end/${i}` })),
});

function input(extra: Partial<BuildInput> = {}): BuildInput {
  return {
    items: [
      item("commands", "New run", { command: true }),
      item("commands", "Theme: dark", { command: true, checked: true }),
      item("commands", "Settings", { command: true }),
      ...Array.from({ length: 8 }, (_, i) => item("runs", `Add a product search ${i}`)),
      item("pipelines", "search-index"),
      item("triggers", "nightly sweep"),
    ],
    selection: [],
    recent: [],
    extensions: [],
    query: "",
    expanded: new Set(),
    icons,
    ...extra,
  };
}

const titles = (b: BuildInput) => buildSections(b).map((s) => s.key);

describe("the palette's groups", () => {
  it("suggests the selection, the recent picks and the commands on an empty query", () => {
    const selection = [item("selection", "Pause", { command: true })];
    const recent = [item("runs", "Add a product search 0")];
    const sections = buildSections(input({ selection, recent }));
    expect(sections.map((s) => s.key)).toEqual(["selection", "recent", "commands"]);
    // The current choice is not a suggestion.
    expect(sections[2].rows.map((r) => (r.kind === "item" ? r.item.label : r.kind))).toEqual(["New run", "Settings"]);
  });

  it("orders the core's groups by their best match and caps them", () => {
    const sections = buildSections(input({ query: "search" }));
    // The shorter, word-start match first.
    expect(sections.map((s) => s.key)).toEqual(["pipelines", "runs"]);
    const runs = sections[1].rows;
    expect(runs).toHaveLength(7);
    expect(runs[6]).toMatchObject({ kind: "more", section: "runs", count: 8 });
    const opened = buildSections(input({ query: "search", expanded: new Set(["runs"]) }));
    expect(opened[1].rows).toHaveLength(8);
    expect(opened[1].rows.every((r) => r.kind === "item")).toBe(true);
  });

  it("keeps the commands under `>`, the extensions' Actions with them", () => {
    expect(parseQuery("> new")).toEqual({ text: "new", commands: true });
    const extensions = [
      {
        extension: { id: "a", name: "Alpha" },
        icon: null,
        actions: [item("ext:a", "New note", { command: true, extension: { id: "a", name: "Alpha" } })],
        sources: [{ key: "a:s", state: ok(3, "new") }],
      },
    ];
    const sections = buildSections(input({ query: ">new", extensions }));
    expect(sections.map((s) => s.key)).toEqual(["commands"]);
    expect(sections[0].rows.map((r) => (r.kind === "item" ? r.item.label : r.kind))).toEqual(["New run", "New note"]);
  });

  it("gives each extension a group: its Actions, its results capped, then its sources' state", () => {
    const extensions = [
      {
        extension: { id: "a", name: "Alpha" },
        icon: "/ext/a/icon.svg",
        actions: [item("ext:a", "Search notes", { extension: { id: "a", name: "Alpha" } })],
        sources: [
          { key: "a:fast", state: ok(8) },
          { key: "a:slow", state: { status: "pending", results: [], total: 0 } as SourceState },
        ],
      },
      {
        extension: { id: "b", name: "Beta" },
        icon: null,
        actions: [],
        sources: [{ key: "b:down", state: { status: "error", results: [], total: 0, error: "down" } as SourceState }],
      },
      { extension: { id: "c", name: "Gamma" }, icon: null, actions: [], sources: [{ key: "c:s", state: ok(0) }] },
    ];
    const sections = buildSections(input({ query: "search", extensions }));
    expect(sections.map((s) => s.key)).toEqual(["pipelines", "runs", "ext:a", "ext:b"]);
    const alpha = sections[2];
    expect(alpha.title).toBe("Alpha");
    expect(alpha.lead).toEqual({ kind: "extension", url: "/ext/a/icon.svg", label: "Alpha" });
    expect(alpha.rows.map((r) => r.kind)).toEqual(["item", ...Array(RESULT_CAP).fill("result"), "more", "state"]);
    const first = alpha.rows[1];
    expect(first.kind === "result" && first.ranges).toEqual([[0, 6]]);
    // A source still searching is shown, not walked through; a failed one is (Enter retries).
    const walked = flatRows(sections);
    expect(walked.some((r) => r.key === "state:a:slow")).toBe(false);
    expect(walked.some((r) => r.key === "state:b:down")).toBe(true);
  });

  it("asks nothing of the extensions' sources on an empty query, and still lists their Actions", () => {
    const extensions = [
      { extension: { id: "a", name: "Alpha" }, icon: null, actions: [item("ext:a", "New note")], sources: [{ key: "a:s", state: undefined }] },
    ];
    expect(titles(input({ extensions }))).toEqual(["commands", "ext:a"]);
  });
});
