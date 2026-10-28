/**
 * The rows of the command field (`shell.paletteRow=plain`): icon, label, the
 * entity a contextual Action acts on, shortcut; an extension's result (status
 * or icon, reference and title, facts on the right); « Show all n »; and the
 * state of a source (searching, or failed with Retry). The groups' headers:
 * their icon and caps title.
 */

import type { ReactNode } from "react";
import { Check, ChevronDown, CircleAlert, LoaderCircle, RotateCcw } from "lucide-react";
import ExtensionIcon from "../../shared/extensions/ExtensionIcon";
import { Kbd, StatusIcon } from "../../shared/ui/kit";
import { rowDomId, type Lead, type PaletteItem, type ResultItem, type Row, type Section } from "./paletteModel";

/** Matched letters (their indexes) in the selection's colour. */
function Highlight({ text, hits }: { text: string; hits: readonly number[] }) {
  if (hits.length === 0) return <>{text}</>;
  const set = new Set(hits);
  const out: ReactNode[] = [];
  let buffer = "";
  let on = false;
  const flush = (at: number) => {
    if (buffer) out.push(on ? <mark key={at}>{buffer}</mark> : buffer);
    buffer = "";
  };
  [...text].forEach((ch, i) => {
    if (set.has(i) !== on) {
      flush(i);
      on = !on;
    }
    buffer += ch;
  });
  flush(text.length);
  return <>{out}</>;
}

function rangeHits(ranges: readonly [number, number][]): number[] {
  const hits: number[] = [];
  for (const [start, end] of ranges) for (let i = start; i < end; i++) hits.push(i);
  return hits;
}

/** A row's or a group's leading mark, in the 14 px icon column. */
export function LeadMark({ lead, size = 14 }: { lead: Lead | null; size?: number }) {
  if (!lead) return <span className="sh-pal__ico" />;
  switch (lead.kind) {
    case "icon": {
      const Icon = lead.icon;
      return (
        <span className="sh-pal__ico">
          <Icon size={size} strokeWidth={1.5} aria-hidden />
        </span>
      );
    }
    case "run":
      return <span className="sh-pal__ico"><StatusIcon kind="run" status={lead.status} size={size} /></span>;
    case "node":
      return <span className="sh-pal__ico"><StatusIcon kind="node" status={lead.status} size={size} /></span>;
    case "tone":
      return <span className="sh-pal__ico"><StatusIcon kind="tone" status={lead.tone} label={lead.label} size={size} /></span>;
    case "extension":
      return (
        <span className="sh-pal__ico">
          <ExtensionIcon url={lead.url} label={lead.label} size={size} />
        </span>
      );
  }
}

interface RowProps {
  row: Row;
  active: boolean;
  /** The group the row sits in: an extension's Action shows its source elsewhere. */
  section: Section;
  /** The icon of each extension's group, by extension id. */
  extensionIcons: ReadonlyMap<string, string | null>;
  onPick: () => void;
  onHover: () => void;
}

/** Which extension a row of another group comes from: its icon, its name as tooltip. */
function SourceMark({ extension, icon }: { extension: { id: string; name: string }; icon: string | null | undefined }) {
  return (
    <span className="sh-pal__src" title={extension.name} aria-label={extension.name} role="img">
      <ExtensionIcon url={icon} label={extension.name} size={12} />
    </span>
  );
}

function ItemRow({ item, hits, ...p }: Omit<RowProps, "row"> & { item: PaletteItem; hits: number[]; id: string }) {
  const foreign = item.extension && p.section.key !== `ext:${item.extension.id}`;
  return (
    <div
      id={p.id}
      role="option"
      aria-selected={p.active}
      className={`sh-pal__row${item.danger ? " sh-pal__row--danger" : ""}`}
      onClick={p.onPick}
      onMouseMove={p.onHover}
      data-testid="palette-row"
    >
      <LeadMark lead={item.lead} />
      <span className="sh-pal__label" title={item.label}>
        <Highlight text={item.label} hits={hits} />
      </span>
      {item.entity && (
        <span className="sh-pal__entity" title={item.entity.label}>
          <LeadMark lead={item.entity.lead} size={12} />
          <span>{item.entity.label}</span>
        </span>
      )}
      <span className="sh-pal__fill" />
      {item.checked ? (
        <Check size={14} strokeWidth={1.5} aria-label="Current" className="sh-pal__check" />
      ) : item.kbd ? (
        <Kbd>{item.kbd}</Kbd>
      ) : null}
      {foreign && item.extension && <SourceMark extension={item.extension} icon={p.extensionIcons.get(item.extension.id)} />}
    </div>
  );
}

function resultLead(item: ResultItem): Lead | null {
  const { status, icon } = item.result;
  if (status) return { kind: "tone", tone: status.tone, label: status.label };
  if (icon) return { kind: "extension", url: icon, label: item.result.title };
  return null;
}

function ResultRow({ item, ranges, ...p }: Omit<RowProps, "row"> & { item: ResultItem; ranges: [number, number][]; id: string }) {
  const r = item.result;
  const tip = [r.ref, r.title, r.meta, r.status?.label].filter(Boolean).join(" · ");
  return (
    <div
      id={p.id}
      role="option"
      aria-selected={p.active}
      className="sh-pal__row"
      title={tip}
      onClick={p.onPick}
      onMouseMove={p.onHover}
      data-testid="palette-result"
    >
      <LeadMark lead={resultLead(item)} />
      {r.ref && <span className="sh-pal__ref">{r.ref}</span>}
      <span className="sh-pal__label">
        <Highlight text={r.title} hits={rangeHits(ranges)} />
      </span>
      <span className="sh-pal__fill" />
      {r.meta && <span className="sh-pal__meta">{r.meta}</span>}
      {p.section.key !== `ext:${item.extension.id}` && (
        <SourceMark extension={item.extension} icon={p.extensionIcons.get(item.extension.id)} />
      )}
    </div>
  );
}

export function PaletteRow(props: RowProps) {
  const { row } = props;
  const id = rowDomId(row.key);
  switch (row.kind) {
    case "item":
      return <ItemRow {...props} id={id} item={row.item} hits={row.hits} />;
    case "result":
      return <ResultRow {...props} id={id} item={row.item} ranges={row.ranges} />;
    case "more":
      return (
        <div
          id={id}
          role="option"
          aria-selected={props.active}
          className="sh-pal__row sh-pal__more"
          onClick={props.onPick}
          onMouseMove={props.onHover}
          data-testid="palette-more"
        >
          <span className="sh-pal__ico">
            <ChevronDown size={14} strokeWidth={1.5} aria-hidden />
          </span>
          <span>Show all {row.count}</span>
        </div>
      );
    case "state":
      if (row.state.status === "pending") {
        return (
          <div className="sh-pal__row sh-pal__state" role="option" aria-disabled="true" aria-selected={false} data-testid="palette-searching">
            <span className="sh-pal__ico">
              <LoaderCircle size={12} strokeWidth={2} className="d-spin" aria-hidden />
            </span>
            <span className="sh-pal__state-text">Searching</span>
          </div>
        );
      }
      return (
        <div
          id={id}
          role="option"
          aria-selected={props.active}
          className="sh-pal__row sh-pal__state"
          aria-label={`${row.extension.name}: search failed, ${row.state.error ?? ""}. Retry`}
          onClick={props.onPick}
          onMouseMove={props.onHover}
          data-testid="palette-failed"
        >
          <span className="sh-pal__ico">
            <CircleAlert size={12} strokeWidth={2} className="sh-pal__state-ico--err" aria-hidden />
          </span>
          <span className="sh-pal__state-text">Search failed</span>
          <span className="sh-pal__state-err" title={row.state.error}>
            {row.state.error}
          </span>
          <span className="sh-pal__fill" />
          {/* The whole row retries (Enter, a click): Retry says so. */}
          <span className="sh-pal__retry" aria-hidden>
            <RotateCcw size={12} strokeWidth={1.5} />
            Retry
          </span>
        </div>
      );
  }
}

/** A group's header: its icon and its caps title. */
export function GroupHeader({ section }: { section: Section }) {
  return (
    <div className="sh-pal__head">
      <LeadMark lead={section.lead} size={12} />
      <span className="d-caps" title={section.title}>
        {section.title}
      </span>
    </div>
  );
}
