import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { Search, SearchX } from "lucide-react";
import { followDeepLink } from "../../shared/extensions/deepLink";
import type { EntityContext } from "../../shared/extensions/contract";
import { usePaletteActions, useSearch, useSearchSources } from "../../shared/extensions/palette";
import { useExtensions } from "../../shared/extensions/store";
import { ConfirmDialog } from "../../shared/ui/kit";
import { FOCUSABLE } from "../../shared/ui/kit/layer";
import { toCss } from "../../shared/ui/zoom";
import { useTheme } from "../../hooks/useTheme";
import { useCompactNodes } from "../../hooks/useCompactNodes";
import { useEditStore } from "../../pipeline/store/editStore";
import { useGestureRightsStore } from "../../run/store/gestureRightsStore";
import { usePaletteHost, type PaletteSelection } from "./paletteHost";
import { actionItem, coreItems, GROUP_ICONS, selectionItems } from "./paletteItems";
import {
  buildSections,
  flatRows,
  parseQuery,
  rowDomId,
  type ExtensionGroup,
  type PaletteItem,
  type ResultItem,
  type Row,
  type Section,
} from "./paletteModel";
import { GroupHeader, PaletteRow } from "./PaletteRows";
import { readRecent, rememberPick } from "./recent";
import { usePalette } from "./paletteStore";

/**
 * The command palette under the top bar (Ctrl+K, or the command field):
 * one query over the Runs, Pipelines, Triggers and commands of the core, the
 * Actions of the extensions (the global ones, and the ones acting on the
 * selected item), and the results of their Sources de recherche, which join
 * their group when they arrive. The arrows walk every group, PageUp and
 * PageDown jump between groups, Enter follows the row; Tab stays in the box;
 * Escape or a click outside closes it, and the focus goes back where it was,
 * else to the command field. A gesture to confirm opens once it closed.
 */
export default function CommandPalette() {
  const open = usePalette((s) => s.open);
  const confirm = usePalette((s) => s.confirm);
  const settle = usePalette((s) => s.settle);
  return (
    <>
      {open && <PaletteBox />}
      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          consequence={confirm.consequence}
          target={confirm.target}
          actionLabel={confirm.actionLabel}
          danger={confirm.danger}
          onClose={settle}
          onConfirm={() => {
            settle();
            confirm.run();
          }}
        />
      )}
    </>
  );
}

/** The selected item, as the extensions' contexts hand it over. */
function selectionContext(selection: PaletteSelection | null): EntityContext | null {
  if (!selection) return null;
  switch (selection.kind) {
    case "run": {
      const { run } = selection;
      return { kind: "run", id: run.id, name: run.name, pipeline: run.pipeline, status: run.status, attributes: run.attributes } as EntityContext;
    }
    case "node": {
      const { node } = selection;
      return { kind: "node", id: node.id, name: node.name, type: node.type, status: node.status, attributes: {} } as EntityContext;
    }
    case "trigger": {
      const t = selection.trigger;
      return {
        kind: "trigger",
        id: t.id,
        name: t.name,
        pipeline: t.pipeline_name,
        enabled: t.enabled,
        attributes: t.attributes ?? {},
        last_editor: t.last_editor ?? null,
        suspension: t.suspension ?? null,
      } as EntityContext;
    }
    case "pipeline": {
      const { pipeline } = selection;
      return { kind: "pipeline", id: pipeline.id, name: pipeline.name, attributes: pipeline.attributes };
    }
  }
}

const EMPTY: ReadonlySet<string> = new Set();
const BOX_WIDTH = 640;
const MARGIN = 16;

/** The box's centre on the command field's, kept inside the window. */
function useFieldAnchor(box: React.RefObject<HTMLDivElement | null>): CSSProperties | undefined {
  const [left, setLeft] = useState<number | null>(null);
  useEffect(() => {
    const field = document.querySelector('[data-testid="command-field"]');
    const scrim = box.current?.parentElement;
    if (!field || !scrim || typeof ResizeObserver === "undefined") return;
    const place = () => {
      const f = field.getBoundingClientRect();
      const r = scrim.getBoundingClientRect();
      const width = toCss(r.width);
      const half = Math.min(BOX_WIDTH, width - 2 * MARGIN) / 2;
      setLeft(Math.max(half + MARGIN, Math.min(width - half - MARGIN, toCss(f.left + f.width / 2 - r.left))));
    };
    const observer = new ResizeObserver(place);
    observer.observe(field);
    observer.observe(scrim);
    return () => observer.disconnect();
  }, [box]);
  return left === null ? undefined : { left };
}

function PaletteBox() {
  const query = usePalette((s) => s.query);
  const setQuery = usePalette((s) => s.setQuery);
  const hide = usePalette((s) => s.hide);
  const ask = usePalette((s) => s.ask);
  const host = usePaletteHost((s) => s.host);
  const pipelines = useEditStore((s) => s.pipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const rights = useGestureRightsStore((s) => s.rights);
  const { theme, mode, setTheme, toggleMode } = useTheme();
  const { compactNodes, toggleCompactNodes } = useCompactNodes();
  const extensions = useExtensions();

  const entity = useMemo(() => selectionContext(host.selection), [host.selection]);
  const { context, global, contextual } = usePaletteActions(entity);
  const sources = useSearchSources(context);
  const parsed = parseQuery(query);
  const { states, retry } = useSearch(sources, parsed.commands ? "" : parsed.text, context);

  const items = useMemo(
    () =>
      coreItems({
        host,
        pipelines,
        openPipeline: (id) => void openPipeline(id),
        appearance: { theme, mode },
        setTheme,
        toggleMode,
        compactNodes,
        toggleCompactNodes,
      }),
    [host, pipelines, openPipeline, theme, mode, setTheme, toggleMode, compactNodes, toggleCompactNodes],
  );
  const selection = useMemo(
    () => [
      ...selectionItems(host.selection, rights, ask, host.openRun),
      ...contextual.map((action) => actionItem(action, host.selection)),
    ],
    [host.selection, host.openRun, rights, ask, contextual],
  );
  const groups = useMemo((): ExtensionGroup[] => {
    return extensions.flatMap(({ entry }): ExtensionGroup[] => {
      const own = sources.filter((s) => s.extension.id === entry.id);
      const mine = global.filter((a) => a.element.extension.id === entry.id);
      if (own.length === 0 && mine.length === 0) return [];
      const ranks = [...own, ...mine.map((a) => a.element)].flatMap((e) => e.contribution.rank ?? []);
      return [
        {
          extension: { id: entry.id, name: entry.name },
          icon: own.find((s) => s.search?.icon)?.search?.icon ?? null,
          rank: ranks.length > 0 ? Math.min(...ranks) : null,
          actions: mine.map((a) => actionItem(a, null)),
          sources: own.map((s) => ({ key: s.key, state: states[s.key] })),
        },
      ];
    });
  }, [extensions, sources, global, states]);
  const extensionIcons = useMemo(() => new Map(groups.map((g) => [g.extension.id, g.icon])), [groups]);

  const [picks] = useState(readRecent);
  const recent = useMemo(
    () =>
      picks.flatMap((pick): (PaletteItem | ResultItem)[] => {
        if (pick.kind !== "result") {
          const item = items.find((i) => i.recent?.kind === pick.kind && i.recent.id === pick.id);
          return item ? [item] : [];
        }
        const ext = extensions.find((e) => e.entry.id === pick.extension);
        if (!ext) return [];
        const result = { id: pick.id, title: pick.title, ref: pick.ref, link: pick.link, avatar: pick.avatar };
        return [{ key: `${pick.extension}:${pick.link}`, extension: { id: ext.entry.id, name: ext.entry.name }, result }];
      }),
    [picks, items, extensions],
  );

  // What « Show all » opened, and the row the keyboard points at, for this query.
  const [expanded, setExpanded] = useState<{ query: string; keys: ReadonlySet<string> }>({ query, keys: EMPTY });
  const opened = expanded.query === query ? expanded.keys : EMPTY;
  const [pointed, setPointed] = useState<{ query: string; key: string | null }>({ query, key: null });

  const input = { items, selection, recent, extensions: groups, query, icons: GROUP_ICONS };
  const sections = buildSections({ ...input, expanded: opened });
  const rows = flatRows(sections);
  const active = rows.find((r) => r.key === (pointed.query === query ? pointed.key : null)) ?? rows[0] ?? null;
  const point = (key: string) => setPointed({ query, key });

  const box = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const anchor = useFieldAnchor(box);

  // The focus: in the field while open; back where it was once closed, else
  // on the command field.
  useEffect(() => {
    const before = document.activeElement;
    field.current?.focus();
    return () => {
      if (before instanceof HTMLElement && before !== document.body && before.isConnected) before.focus();
      else document.querySelector<HTMLElement>('[data-testid="command-field"]')?.focus();
    };
  }, []);

  const activeKey = active?.key ?? null;
  useEffect(() => {
    if (!activeKey) return;
    const el = box.current?.querySelector(`#${rowDomId(activeKey)}`);
    el?.scrollIntoView?.({ block: "nearest" });
  }, [activeKey]);

  const move = (step: number) => {
    if (rows.length === 0) return;
    const at = active ? rows.indexOf(active) : -1;
    point(rows[Math.max(0, Math.min(rows.length - 1, at + step))].key);
  };
  const jumpGroup = (step: number) => {
    const firsts = sections.map((s) => rows.find((r) => s.rows.includes(r))).filter((r): r is Row => !!r);
    const at = sections.findIndex((s) => s.rows.some((r) => r.key === active?.key));
    const current = at >= 0 ? firsts.indexOf(rows.find((r) => sections[at].rows.includes(r))!) : -1;
    const next = firsts[Math.max(0, Math.min(firsts.length - 1, current + step))];
    if (next) point(next.key);
  };

  const pick = (row: Row | null) => {
    if (!row) return;
    // CORE-205: a refused gesture says why on its row, and does nothing.
    if (row.kind === "item" && row.item.disabledReason) return;
    if (row.kind === "more") {
      // The active row stays in place: the first of the rows it reveals.
      const at = rows.indexOf(row);
      const keys = new Set(opened).add(row.section);
      setExpanded({ query, keys });
      const next = flatRows(buildSections({ ...input, expanded: keys }))[at];
      if (next) point(next.key);
      return;
    }
    if (row.kind === "state") {
      retry(row.source);
      return;
    }
    hide();
    if (row.kind === "item") {
      if (row.item.recent) rememberPick(row.item.recent);
      row.item.run();
    } else {
      const { extension, result } = row.item;
      rememberPick({
        kind: "result",
        id: result.id,
        extension: extension.id,
        title: result.title,
        ref: result.ref,
        link: result.link,
        avatar: result.avatar,
      });
      followDeepLink(result.link);
    }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.nativeEvent.isComposing) return;
    switch (e.key) {
      case "ArrowDown":
        move(1);
        break;
      case "ArrowUp":
        move(-1);
        break;
      case "PageDown":
        jumpGroup(1);
        break;
      case "PageUp":
        jumpGroup(-1);
        break;
      case "Enter":
        pick(active);
        break;
      case "Escape":
        // Consumed: the surface open under the bar (CORE-183) stays open.
        e.preventDefault();
        hide();
        break;
      case "Tab": {
        // The focus stays in the box: the field, then each Retry.
        const focusable = [...(box.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])];
        if (focusable.length === 0) break;
        const at = focusable.indexOf(document.activeElement as HTMLElement);
        const next = (at + (e.shiftKey ? -1 : 1) + focusable.length) % focusable.length;
        focusable[next].focus();
        break;
      }
      default:
        return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  const nothing = sections.length === 0;
  return (
    <div className="sh-pal-scrim" onMouseDown={hide} data-testid="command-palette-scrim">
      <div
        ref={box}
        className="sh-pal"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        data-testid="command-palette"
        style={anchor}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={onKeyDown}
      >
        <div className="sh-pal__field">
          <Search size={14} strokeWidth={1.5} aria-hidden />
          <input
            ref={field}
            className="sh-pal__input"
            placeholder="Search, or > for commands"
            aria-label="Search runs, pipelines, triggers, commands and extensions"
            aria-controls="sh-pal-list"
            aria-activedescendant={active ? rowDomId(active.key) : undefined}
            role="combobox"
            aria-expanded="true"
            autoComplete="off"
            spellCheck={false}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            data-testid="command-palette-input"
          />
        </div>
        {/* A click on a row keeps the focus in the field: the keys stay the palette's. */}
        <div
          className="sh-pal__list"
          id="sh-pal-list"
          role="listbox"
          aria-label="Results"
          onMouseDown={(e) => e.preventDefault()}
        >
          {nothing && (
            <div className="sh-pal__empty" data-testid="command-palette-empty">
              <SearchX size={14} strokeWidth={1.5} aria-hidden />
              No match
            </div>
          )}
          {sections.map((section: Section) => (
            <div
              key={section.key}
              role="group"
              aria-label={section.title}
              className="sh-pal__sec"
              data-testid={`palette-group-${section.key}`}
            >
              <GroupHeader section={section} />
              {section.rows.map((row) => (
                <PaletteRow
                  key={row.key}
                  row={row}
                  section={section}
                  active={row.key === active?.key}
                  extensionIcons={extensionIcons}
                  onPick={() => pick(row)}
                  onHover={() => {
                    if (row.key !== active?.key) point(row.key);
                  }}
                />
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
